#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij <jelmer@debian.org>
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

import json
import logging
import operator
import os
import re
import socket
import urllib.error
from typing import Optional, Iterable
from urllib.parse import quote, urlparse, urlunparse, urljoin
from urllib.request import urlopen, Request

from .vcs import (
    unsplit_vcs_url,
    browse_url_from_repo_url,
    plausible_url as plausible_vcs_url,
    plausible_browse_url as plausible_vcs_browse_url,
    sanitize_url as sanitize_vcs_url,
    is_gitlab_site,
    guess_repo_from_url,
    verify_repository_url,
    )

from . import (
    DEFAULT_URLLIB_TIMEOUT,
    USER_AGENT,
    UpstreamDatum,
    min_certainty,
    certainty_to_confidence,
    certainty_sufficient,
    _load_json_url,
    Person,
    )


# Pecl is quite slow, so up the timeout a bit.
PECL_URLLIB_TIMEOUT = 15


logger = logging.getLogger(__name__)


class NoSuchSourceForgeProject(Exception):

    def __init__(self, project):
        self.project = project


def get_sf_metadata(project):
    try:
        return _load_json_url(
            'https://sourceforge.net/rest/p/%s' % project)
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        raise NoSuchSourceForgeProject(project)


class NoSuchRepologyProject(Exception):

    def __init__(self, project):
        self.project = project


def get_repology_metadata(srcname, repo='debian_unstable'):
    url = ('https://repology.org/tools/project-by?repo=%s&name_type=srcname'
           '&target_page=api_v1_project&name=%s' % (repo, srcname))
    try:
        return _load_json_url(url)
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        raise NoSuchRepologyProject(srcname)


DATUM_TYPES = {
    'Bug-Submit': str,
    'Bug-Database': str,
    'Repository': str,
    'Repository-Browse': str,
    'X-License': str,
    'X-Summary': str,
    'X-Description': str,
    'X-Wiki': str,
    'X-SourceForge-Project': str,
    'Archive': str,
    'Homepage': str,
    'Name': str,
    'X-Version': str,
    'X-Download': str,
    'X-Pecl-URL': str,
    'Screenshots': list,
    'Contact': str,
    'X-Maintainer': Person,
    }


def known_bad_guess(datum):  # noqa: C901
    try:
        expected_type = DATUM_TYPES[datum.field]
    except KeyError:
        if datum.field.startswith('X-'):
            logging.debug('Unknown field %s', datum.field)
        else:
            logging.warning('Unknown field %s', datum.field)
        return False
    if not isinstance(datum.value, expected_type):
        logging.warning(
            'filtering out bad value %r for %s',
            datum.value, datum.field)
        return True
    if datum.field in ('Bug-Submit', 'Bug-Database'):
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'bugzilla.gnome.org':
            return True
        if parsed_url.hostname == 'bugs.freedesktop.org':
            return True
    if datum.field == 'Repository':
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'anongit.kde.org':
            return True
        if parsed_url.hostname == 'git.gitorious.org':
            return True
    if datum.field == 'Homepage':
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname in ('pypi.org', 'rubygems.org'):
            return True
    if datum.field == 'Repository-Browse':
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'cgit.kde.org':
            return True
    if datum.field == 'Name':
        if datum.value.lower() == 'package':
            return True
    if datum.field == 'X-Version':
        if datum.value.lower() in ('devel', ):
            return True
    if isinstance(datum.value, str) and datum.value.lower() == 'unknown':
        return True
    return False


def filter_bad_guesses(
        guessed_items: Iterable[UpstreamDatum]) -> Iterable[UpstreamDatum]:
    return filter(lambda x: not known_bad_guess(x), guessed_items)


def update_from_guesses(upstream_metadata, guessed_items):
    changed = False
    for datum in guessed_items:
        current_datum = upstream_metadata.get(datum.field)
        if not current_datum or (
                certainty_to_confidence(datum.certainty) <
                certainty_to_confidence(current_datum.certainty)):
            upstream_metadata[datum.field] = datum
            changed = True
    return changed


def guess_from_debian_rules(path, trust_package):
    from debmutate._rules import Makefile
    mf = Makefile.from_path(path)
    try:
        upstream_git = mf.get_variable(b'UPSTREAM_GIT')
    except KeyError:
        pass
    else:
        yield UpstreamDatum(
            "Repository", upstream_git.decode(), "likely")
    try:
        upstream_url = mf.get_variable(b'DEB_UPSTREAM_URL')
    except KeyError:
        pass
    else:
        yield UpstreamDatum("X-Download", upstream_url.decode(), "likely")


def guess_from_debian_watch(path, trust_package):
    from debmutate.watch import (
        parse_watch_file,
        MissingVersion,
        )

    def get_package_name():
        from debian.deb822 import Deb822
        with open(os.path.join(os.path.dirname(path), 'control'), 'r') as f:
            return Deb822(f)['Source']
    with open(path, 'r') as f:
        try:
            wf = parse_watch_file(f)
        except MissingVersion:
            return
        if not wf:
            return
        for w in wf:
            url = w.format_url(package=get_package_name)
            if 'mode=git' in w.options:
                yield UpstreamDatum(
                    "Repository", url, "confident",
                    origin=path)
                continue
            if 'mode=svn' in w.options:
                yield UpstreamDatum(
                    "Repository", url, "confident",
                    origin=path)
                continue
            if url.startswith('https://') or url.startswith('http://'):
                repo = guess_repo_from_url(url)
                if repo:
                    yield UpstreamDatum(
                        "Repository", repo, "likely",
                        origin=path)
                    continue
            m = re.match('https?://sf.net/([^/]+)', url)
            if m:
                yield UpstreamDatum(
                    "Archive", "SourceForge", "certain",
                    origin=path)
                yield UpstreamDatum(
                    "X-SourceForge-Project", m.group(1), "certain",
                    origin=path)
                continue
            m = re.match(
                'https?://hackage.haskell.org/package/(.*)/distro-monitor',
                url)
            if m:
                yield UpstreamDatum(
                    "Archive", "Hackage", "certain", origin=path)
                yield UpstreamDatum(
                    "X-Hackage-Package", m.group(1), "certain", origin=path)


def guess_from_debian_control(path, trust_package):
    with open(path, 'r') as f:
        from debian.deb822 import Deb822
        control = Deb822(f)
    if 'Homepage' in control:
        yield UpstreamDatum('Homepage', control['Homepage'], 'certain')
    if 'XS-Go-Import-Path' in control:
        yield (
            UpstreamDatum(
                'Repository',
                'https://' + control['XS-Go-Import-Path'],
                'likely'))
    if 'Description' in control:
        yield UpstreamDatum(
            'X-Summary', control['Description'].splitlines(False)[0], 'certain')
        yield UpstreamDatum(
            'X-Description',
            ''.join(control['Description'].splitlines(True)[1:]), 'certain')


def guess_from_debian_changelog(path, trust_package):
    from debian.changelog import Changelog
    with open(path, 'rb') as f:
        cl = Changelog(f)
    source = cl.package
    if source.startswith('rust-'):
        try:
            from toml.decoder import load as load_toml
            with open('debian/debcargo.toml', 'r') as f:
                debcargo = load_toml(f)
        except FileNotFoundError:
            semver_suffix = False
        else:
            semver_suffix = debcargo.get('semver_suffix')
        from debmutate.debcargo import parse_debcargo_source_name, cargo_translate_dashes
        crate, crate_semver_version = parse_debcargo_source_name(
            source, semver_suffix)
        if '-' in crate:
            crate = cargo_translate_dashes(crate)
        yield UpstreamDatum('Archive', 'crates.io', 'certain')
        yield UpstreamDatum('X-Cargo-Crate', crate, 'certain')


def guess_from_python_metadata(pkg_info):
    if 'Name' in pkg_info:
        yield UpstreamDatum('Name', pkg_info['name'], 'certain')
    if 'Version' in pkg_info:
        yield UpstreamDatum('X-Version', pkg_info['Version'], 'certain')
    if 'Home-Page' in pkg_info:
        repo = guess_repo_from_url(pkg_info['Home-Page'])
        if repo:
            yield UpstreamDatum(
                'Repository', repo, 'likely')
    for value in pkg_info.get_all('Project-URL', []):
        url_type, url = value.split(', ')
        if url_type in ('GitHub', 'Repository', 'Source Code'):
            yield UpstreamDatum(
                'Repository', url, 'certain')
        if url_type in ('Bug Tracker', ):
            yield UpstreamDatum(
                'Bug-Database', url, 'certain')
    if 'Summary' in pkg_info:
        yield UpstreamDatum('X-Summary', pkg_info['Summary'], 'certain')
    if 'Author' in pkg_info:
        author_email = pkg_info.get('Author-email')
        author = Person(pkg_info['Author'], author_email)
        yield UpstreamDatum('X-Authors', [author], 'certain')
    if 'License' in pkg_info:
        yield UpstreamDatum('X-License', pkg_info['License'], 'certain')
    if 'Download-URL' in pkg_info:
        yield UpstreamDatum('X-Download', pkg_info['Download-URL'], 'certain')
    yield from parse_python_long_description(
        pkg_info.get_payload(), pkg_info.get_content_type())


def guess_from_pkg_info(path, trust_package):
    """Get the metadata from a PKG-INFO file."""
    from email.parser import Parser
    try:
        with open(path, 'r') as f:
            pkg_info = Parser().parse(f)
    except FileNotFoundError:
        return
    yield from guess_from_python_metadata(pkg_info)


def parse_python_long_description(long_description, content_type):
    if long_description in (None, ''):
        return
    # Discard encoding, etc.
    if content_type:
        content_type = content_type.split(';')[0]
    if content_type in (None, 'text/plain'):
        if len(long_description.splitlines()) > 30:
            return
        yield UpstreamDatum(
            'X-Description', long_description, 'possible')
        extra_md = []
    elif content_type in ('text/restructured-text', 'text/x-rst'):
        from .readme import description_from_readme_rst
        description, extra_md = description_from_readme_rst(long_description)
        if description:
            yield UpstreamDatum('X-Description', description, 'possible')
    elif content_type == 'text/markdown':
        from .readme import description_from_readme_md
        description, extra_md = description_from_readme_md(long_description)
        if description:
            yield UpstreamDatum('X-Description', description, 'possible')
    else:
        extra_md = []
    for datum in extra_md:
        yield datum


def guess_from_setup_cfg(path, trust_package):
    from setuptools.config import read_configuration
    # read_configuration needs a function cwd
    try:
        os.getcwd()
    except FileNotFoundError:
        os.chdir(os.path.dirname(path))
    config = read_configuration(path)
    metadata = config.get('metadata')
    if metadata:
        if 'name' in metadata:
            yield UpstreamDatum('Name', metadata['name'], 'certain')
        if 'url' in metadata:
            yield from parse_python_url(metadata['url'])
        yield from parse_python_long_description(
            metadata.get('long_description'),
            metadata.get('long_description_content_type'))
        if 'description' in metadata:
            yield UpstreamDatum('X-Summary', metadata['description'], 'certain')


def parse_python_url(url):
    repo = guess_repo_from_url(url)
    if repo:
        yield UpstreamDatum('Repository', repo, 'likely')
    yield UpstreamDatum('Homepage', url, 'likely')


def guess_from_setup_py_executed(path):
    from distutils.core import run_setup
    result = run_setup(os.path.abspath(path), stop_after="init")
    if result.get_name() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum('Name', result.get_name(), 'certain')
    if result.get_version() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum('X-Version', result.get_version(), 'certain')
    if result.get_url() not in (None, '', 'UNKNOWN'):
        yield from parse_python_url(result.get_url())
    if result.get_download_url() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum(
            'X-Download', result.get_download_url(), 'likely')
    if result.get_license() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum(
            'X-License', result.get_license(), 'likely')
    if result.get_contact() not in (None, '', 'UNKNOWN'):
        contact = result.get_contact()
        if result.get_contact_email() not in (None, '', 'UNKNOWN'):
            contact += " <%s>" % result.get_contact_email()
        yield UpstreamDatum('Contact', contact, 'likely')
    if result.get_description() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum('X-Summary', result.get_description(), 'certain')
    yield from parse_python_long_description(
        result.metadata.long_description,
        getattr(result.metadata, 'long_description_content_type', None))
    yield from parse_python_project_urls(getattr(result.metadata, 'project_urls', {}))


def parse_python_project_urls(urls):
    for url_type, url in urls.items():
        if url_type in ('GitHub', 'Repository', 'Source Code'):
            yield UpstreamDatum(
                'Repository', url, 'certain')
        if url_type in ('Bug Tracker', ):
            yield UpstreamDatum(
                'Bug-Database', url, 'certain')


def guess_from_setup_py(path, trust_package):
    if trust_package:
        try:
            return guess_from_setup_py_executed(path)
        except Exception as e:
            logging.warning('Failed to run setup.py: %r', e)
    with open(path) as inp:
        setup_text = inp.read()
    import ast

    # Based on pypi.py in https://github.com/nexB/scancode-toolkit/blob/develop/src/packagedcode/pypi.py
    #
    # Copyright (c) nexB Inc. and others. All rights reserved.
    # ScanCode is a trademark of nexB Inc.
    # SPDX-License-Identifier: Apache-2.0

    try:
        tree = ast.parse(setup_text)
    except SyntaxError as e:
        logging.warning('Syntax error while parsing setup.py: %s', e)
        return
    setup_args = {}

    for statement in tree.body:
        # We only care about function calls or assignments to functions named
        # `setup` or `main`
        if (isinstance(statement, (ast.Expr, ast.Call, ast.Assign))
            and isinstance(statement.value, ast.Call)
            and isinstance(statement.value.func, ast.Name)
            # we also look for main as sometimes this is used instead of setup()
            and statement.value.func.id in ('setup', 'main')
        ):

            # Process the arguments to the setup function
            for kw in getattr(statement.value, 'keywords', []):
                arg_name = kw.arg

                if isinstance(kw.value, (ast.Str, ast.Constant)):
                    setup_args[arg_name] = kw.value.s

                elif isinstance(kw.value, (ast.List, ast.Tuple, ast.Set,)):
                    # We collect the elements of a list if the element
                    # and tag function calls
                    value = [
                        elt.s for elt in kw.value.elts
                        if isinstance(elt, ast.Constant)
                    ]
                    setup_args[arg_name] = value

                elif isinstance(kw.value, ast.Dict):
                    setup_args[arg_name] = {}
                    for (key, value) in zip(kw.value.keys, kw.value.values):
                        if isinstance(value, (ast.Str, ast.Constant)):
                            setup_args[key.s] = value.s

                # TODO: what if kw.value is an expression like a call to
                # version=get_version or version__version__

    # End code from https://github.com/nexB/scancode-toolkit/blob/develop/src/packagedcode/pypi.py

    if 'name' in setup_args:
        yield UpstreamDatum('Name', setup_args['name'], 'certain')
    if 'version' in setup_args:
        yield UpstreamDatum('X-Version', setup_args['version'], 'certain')
    if 'description' in setup_args:
        yield UpstreamDatum('X-Summary', setup_args['description'], 'certain')
    if 'long_description' in setup_args:
        yield from parse_python_long_description(
            setup_args['long_description'], setup_args.get('long_description_content_type'))
    if 'license' in setup_args:
        yield UpstreamDatum('X-License', setup_args['license'], 'certain')
    if 'download_url' in setup_args:
        yield UpstreamDatum('X-Download', setup_args['download_url'], 'certain')
    if 'url' in setup_args:
        yield from parse_python_url(setup_args['url'])
    if 'project_urls' in setup_args:
        yield from parse_python_project_urls(setup_args['project_urls'])
    if 'maintainer' in setup_args:
        maintainer_email = setup_args.get('maintainer_email')
        maintainer = Person(setup_args['maintainer'], maintainer_email)
        yield UpstreamDatum('X-Maintainer', maintainer, 'certain')


def guess_from_composer_json(path, trust_package):
    # https://getcomposer.org/doc/04-schema.md
    with open(path, 'r') as f:
        package = json.load(f)
    if 'name' in package:
        yield UpstreamDatum('Name', package['name'], 'certain')
    if 'homepage' in package:
        yield UpstreamDatum('Homepage', package['homepage'], 'certain')
    if 'description' in package:
        yield UpstreamDatum('X-Summary', package['description'], 'certain')
    if 'license' in package:
        yield UpstreamDatum('X-License', package['license'], 'certain')
    if 'version' in package:
        yield UpstreamDatum('X-Version', package['version'], 'certain')


def guess_from_package_json(path, trust_package):
    # see https://docs.npmjs.com/cli/v7/configuring-npm/package-json
    with open(path, 'r') as f:
        package = json.load(f)
    if 'name' in package:
        yield UpstreamDatum('Name', package['name'], 'certain')
    if 'homepage' in package:
        yield UpstreamDatum('Homepage', package['homepage'], 'certain')
    if 'description' in package:
        yield UpstreamDatum('X-Summary', package['description'], 'certain')
    if 'license' in package:
        yield UpstreamDatum('X-License', package['license'], 'certain')
    if 'version' in package:
        yield UpstreamDatum('X-Version', package['version'], 'certain')
    if 'repository' in package:
        if isinstance(package['repository'], dict):
            repo_url = package['repository'].get('url')
        elif isinstance(package['repository'], str):
            repo_url = package['repository']
        else:
            repo_url = None
        if repo_url:
            parsed_url = urlparse(repo_url)
            if parsed_url.scheme and parsed_url.netloc:
                yield UpstreamDatum(
                    'Repository', repo_url, 'certain')
            elif repo_url.startswith('github:'):
                # Some people seem to default to github. :(
                repo_url = 'https://github.com/' + repo_url.split(':', 1)[1]
                yield UpstreamDatum('Repository', repo_url, 'likely')
            else:
                # Some people seem to default to github. :(
                repo_url = 'https://github.com/' + parsed_url.path
                yield UpstreamDatum(
                    'Repository', repo_url, 'likely')
    if 'bugs' in package:
        if isinstance(package['bugs'], dict):
            url = package['bugs'].get('url')
            if url is None and package['bugs'].get('email'):
                url = 'mailto:' + package['bugs']['email']
        else:
            url = package['bugs']
        if url:
            yield UpstreamDatum('Bug-Database', url, 'certain')


def xmlparse_simplify_namespaces(path, namespaces):
    import xml.etree.ElementTree as ET
    namespaces = ['{%s}' % ns for ns in namespaces]
    tree = ET.iterparse(path)
    for _, el in tree:
        for namespace in namespaces:
            el.tag = el.tag.replace(namespace, '')
    return tree.root


def guess_from_package_xml(path, trust_package):
    # https://pear.php.net/manual/en/guide.developers.package2.dependencies.php
    import xml.etree.ElementTree as ET
    try:
        root = xmlparse_simplify_namespaces(path, [
            'http://pear.php.net/dtd/package-2.0',
            'http://pear.php.net/dtd/package-2.1'])
    except ET.ParseError as e:
        logging.warning('Unable to parse package.xml: %s', e)
        return
    assert root.tag == 'package', 'root tag is %r' % root.tag
    name_tag = root.find('name')
    if name_tag is not None:
        yield UpstreamDatum('Name', name_tag.text, 'certain')
    summary_tag = root.find('summary')
    if summary_tag is not None:
        yield UpstreamDatum('X-Summary', summary_tag.text, 'certain')
    description_tag = root.find('description')
    if description_tag is not None:
        yield UpstreamDatum('X-Description', description_tag.text, 'certain')
    version_tag = root.find('version')
    if version_tag is not None:
        release_tag = version_tag.find('release')
        if release_tag is not None:
            yield UpstreamDatum('X-Version', release_tag.text, 'certain')
    license_tag = root.find('license')
    if license_tag is not None:
        yield UpstreamDatum('X-License', license_tag.text, 'certain')
    for url_tag in root.findall('url'):
        if url_tag.get('type') == 'repository':
            yield UpstreamDatum(
                'Repository', url_tag.text, 'certain')
        if url_tag.get('type') == 'bugtracker':
            yield UpstreamDatum('Bug-Database', url_tag.text, 'certain')


def guess_from_pod(contents):
    # See https://perldoc.perl.org/perlpod
    by_header = {}
    inheader = None
    for line in contents.splitlines(True):
        if line.startswith(b'=head1 '):
            inheader = line.rstrip(b'\n').split(b' ', 1)[1]
            by_header[inheader.decode('utf-8', 'surrogateescape').upper()] = ''
        elif inheader:
            by_header[inheader.decode('utf-8', 'surrogateescape').upper()] += line.decode('utf-8', 'surrogateescape')

    if 'DESCRIPTION' in by_header:
        description = by_header['DESCRIPTION'].lstrip('\n')
        description = re.sub(r'[FXZSCBI]\<([^>]+)>', r'\1', description)
        description = re.sub(r'L\<([^\|]+)\|([^\>]+)\>', r'\2', description)
        description = re.sub(r'L\<([^\>]+)\>', r'\1', description)
        # TODO(jelmer): Support E<>
        yield UpstreamDatum('X-Description', description, 'likely')

    if 'NAME' in by_header:
        lines = by_header['NAME'].strip().splitlines()
        if lines:
            name = lines[0]
            if ' - ' in name:
                (name, summary) = name.split(' - ', 1)
                yield UpstreamDatum('Name', name.strip(), 'confident')
                yield UpstreamDatum('X-Summary', summary.strip(), 'confident')
            elif ' ' not in name:
                yield UpstreamDatum('Name', name.strip(), 'confident')


def guess_from_perl_module(path):
    import subprocess
    try:
        stdout = subprocess.check_output(['perldoc', '-u', path])
    except subprocess.CalledProcessError:
        logging.warning('Error running perldoc, skipping.')
        return
    yield from guess_from_pod(stdout)


def guess_from_perl_dist_name(path, dist_name):
    mod_path = os.path.join(
        os.path.dirname(path), 'lib', dist_name.replace('-', '/') + '.pm')
    if os.path.exists(mod_path):
        yield from guess_from_perl_module(mod_path)


def guess_from_dist_ini(path, trust_package):
    from configparser import (
        RawConfigParser,
        NoSectionError,
        NoOptionError,
        ParsingError,
        )
    parser = RawConfigParser(strict=False)
    with open(path, 'r') as f:
        try:
            parser.read_string('[START]\n' + f.read())
        except ParsingError as e:
            logging.warning('Unable to parse dist.ini: %r', e)
    try:
        dist_name = parser['START']['name']
    except (NoSectionError, NoOptionError, KeyError):
        dist_name = None
    else:
        yield UpstreamDatum('Name', dist_name, 'certain')
    try:
        yield UpstreamDatum('X-Version', parser['START']['version'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield UpstreamDatum('X-Summary', parser['START']['abstract'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield UpstreamDatum(
            'Bug-Database', parser['MetaResources']['bugtracker.web'],
            'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield UpstreamDatum(
            'Repository', parser['MetaResources']['repository.url'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield UpstreamDatum(
            'X-License', parser['START']['license'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        copyright = '%s %s' % (
            parser['START']['copyright_year'],
            parser['START']['copyright_holder'],
        )
    except (NoSectionError, NoOptionError, KeyError):
        pass
    else:
        yield UpstreamDatum('X-Copyright', copyright, 'certain')

    # Wild guess:
    if dist_name:
        yield from guess_from_perl_dist_name(path, dist_name)


def guess_from_debian_copyright(path, trust_package):
    from debian.copyright import (
        Copyright,
        NotMachineReadableError,
        MachineReadableFormatError,
        )
    with open(path, 'r') as f:
        try:
            copyright = Copyright(f, strict=False)
        except NotMachineReadableError:
            header = None
        except MachineReadableFormatError as e:
            logging.warning('Error parsing copyright file: %s', e)
            header = None
        except ValueError as e:
            # This can happen with an error message of
            # ValueError: value must not have blank lines
            logging.warning('Error parsing copyright file: %s', e)
            header = None
        else:
            header = copyright.header
    if header:
        if header.upstream_name:
            yield UpstreamDatum("Name", header.upstream_name, 'certain')
        if header.upstream_contact:
            yield UpstreamDatum(
                "Contact", ','.join(header.upstream_contact), 'certain')
        if header.source:
            if ' ' in header.source:
                from_urls = [u for u in re.split('[ ,\n]', header.source) if u]
            else:
                from_urls = [header.source]
            for from_url in from_urls:
                repo_url = guess_repo_from_url(from_url)
                if repo_url:
                    yield UpstreamDatum(
                        'Repository', repo_url, 'likely')
                if (from_url.startswith('https://pecl.php.net/package/') or
                        from_url.startswith('http://pecl.php.net/package/')):
                    yield UpstreamDatum('X-Pecl-URL', from_url, 'certain')
        if "X-Upstream-Bugs" in header:
            yield UpstreamDatum(
                "Bug-Database", header["X-Upstream-Bugs"], 'certain')
        if "X-Source-Downloaded-From" in header:
            url = guess_repo_from_url(header["X-Source-Downloaded-From"])
            if url is not None:
                yield UpstreamDatum("Repository", url, 'certain')


def url_from_git_clone_command(command):
    import shlex
    argv = shlex.split(command.decode('utf-8', 'surrogateescape'))
    args = [arg for arg in argv if arg.strip()]
    i = 0
    while i < len(args):
        if not args[i].startswith('-'):
            i += 1
            continue
        if '=' in args[i]:
            del args[i]
            continue
        # arguments that take a parameter
        if args[i] in ('-b', '--depth', '--branch'):
            del args[i]
            del args[i]
            continue
        del args[i]
    try:
        url = args[2]
    except IndexError:
        url = args[0]
    if plausible_vcs_url(url):
        return url
    return None


def url_from_fossil_clone_command(command):
    import shlex
    argv = shlex.split(command.decode('utf-8', 'surrogateescape'))
    args = [arg for arg in argv if arg.strip()]
    i = 0
    while i < len(args):
        if not args[i].startswith('-'):
            i += 1
            continue
        if '=' in args[i]:
            del args[i]
            continue
        del args[i]
    try:
        url = args[2]
    except IndexError:
        url = args[0]
    if plausible_vcs_url(url):
        return url
    return None


def guess_from_readme(path, trust_package):  # noqa: C901
    urls = []
    try:
        with open(path, 'rb') as f:
            lines = list(f.readlines())
            for i, line in enumerate(lines):
                line = line.strip()
                cmdline = line.strip().lstrip(b'$').strip()
                if (cmdline.startswith(b'git clone ') or
                        cmdline.startswith(b'fossil clone ')):
                    while cmdline.endswith(b'\\'):
                        cmdline += lines[i+1]
                        cmdline = cmdline.strip()
                        i += 1
                    if cmdline.startswith(b'git clone '):
                        url = url_from_git_clone_command(cmdline)
                    elif cmdline.startswith(b'fossil clone '):
                        url = url_from_fossil_clone_command(cmdline)
                    if url:
                        urls.append(url)
                project_re = b'([^/]+)/([^/?.()"#>\\s]*[^-/?.()"#>\\s])'
                for m in re.finditer(
                        b'https://travis-ci.org/' + project_re, line):
                    yield UpstreamDatum(
                        'Repository', 'https://github.com/%s/%s' % (
                            m.group(1).decode(), m.group(2).decode().rstrip()),
                        'possible')
                for m in re.finditer(
                        b'https://coveralls.io/r/' + project_re, line):
                    yield UpstreamDatum(
                        'Repository', 'https://github.com/%s/%s' % (
                            m.group(1).decode(), m.group(2).decode().rstrip()),
                        'possible')
                for m in re.finditer(
                        b'https://github.com/([^/]+)/([^/]+)/issues', line):
                    yield UpstreamDatum(
                        'Bug-Database',
                        m.group(0).decode().rstrip(), 'possible')
                for m in re.finditer(
                        b'https://github.com/' + project_re + b'(.git)?',
                        line):
                    yield UpstreamDatum(
                        'Repository',
                        m.group(0).rstrip(b'.').decode().rstrip(),
                        'possible')
                m = re.fullmatch(
                    b'https://github.com/' + project_re, line)
                if m:
                    yield UpstreamDatum(
                        'Repository',
                        line.strip().rstrip(b'.').decode(), 'possible')
                m = re.fullmatch(b'git://([^ ]+)', line)
                if m:
                    yield UpstreamDatum(
                        'Repository',
                        line.strip().rstrip(b'.').decode(), 'possible')
                for m in re.finditer(
                        b'https://([^]/]+)/([^]\\s()"#]+)', line):
                    if is_gitlab_site(m.group(1).decode()):
                        url = m.group(0).rstrip(b'.').decode().rstrip()
                        try:
                            repo_url = guess_repo_from_url(url)
                        except ValueError:
                            logger.warning(
                                'Ignoring invalid URL %s in %s', url, path)
                        else:
                            if repo_url:
                                yield UpstreamDatum(
                                    'Repository', repo_url, 'possible')
        if path.lower().endswith('readme.md'):
            with open(path, 'rb') as f:
                from .readme import description_from_readme_md
                contents = f.read().decode('utf-8', 'surrogateescape')
                description, extra_md = description_from_readme_md(contents)
        elif path.lower().endswith('readme.rst'):
            with open(path, 'rb') as f:
                from .readme import description_from_readme_rst
                contents = f.read().decode('utf-8', 'surrogateescape')
                description, extra_md = description_from_readme_rst(contents)
        elif path.lower().endswith('readme'):
            with open(path, 'rb') as f:
                from .readme import description_from_readme_plain
                contents = f.read().decode('utf-8', 'surrogateescape')
                description, extra_md = description_from_readme_plain(contents)
        else:
            description = None
            extra_md = []
        if description is not None:
            yield UpstreamDatum(
                'X-Description', description, 'possible')
        for datum in extra_md:
            yield datum
        if path.lower().endswith('readme.pod'):
            with open(path, 'rb') as f:
                yield from guess_from_pod(f.read())
    except IsADirectoryError:
        pass

    def prefer_public(url):
        parsed_url = urlparse(url)
        if 'ssh' in parsed_url.scheme:
            return 1
        return 0
    urls.sort(key=prefer_public)
    if urls:
        yield UpstreamDatum('Repository', urls[0], 'possible')


def guess_from_debian_patch(path, trust_package):
    with open(path, 'rb') as f:
        for line in f:
            if line.startswith(b'Forwarded: '):
                forwarded = line.split(b':', 1)[1].strip()
                bug_db = bug_database_from_issue_url(forwarded.decode('utf-8'))
                if bug_db:
                    yield UpstreamDatum('Bug-Database', bug_db, 'possible')
                repo_url = repo_url_from_merge_request_url(
                    forwarded.decode('utf-8'))
                if repo_url:
                    yield UpstreamDatum('Repository', repo_url, 'possible')


def guess_from_meta_json(path, trust_package):
    with open(path, 'r') as f:
        data = json.load(f)
        if 'name' in data:
            dist_name = data['name']
            yield UpstreamDatum('Name', data['name'], 'certain')
        else:
            dist_name = None
        if 'version' in data:
            version = str(data['version'])
            if version.startswith('v'):
                version = version[1:]
            yield UpstreamDatum('X-Version', version, 'certain')
        if 'abstract' in data:
            yield UpstreamDatum('X-Summary', data['abstract'], 'certain')
        if 'resources' in data:
            resources = data['resources']
            if 'bugtracker' in resources and 'web' in resources['bugtracker']:
                yield UpstreamDatum(
                    "Bug-Database", resources["bugtracker"]["web"], 'certain')
                # TODO(jelmer): Support resources["bugtracker"]["mailto"]
            if 'homepage' in resources:
                yield UpstreamDatum(
                    "Homepage", resources["homepage"], 'certain')
            if 'repository' in resources:
                repo = resources['repository']
                if 'url' in repo:
                    yield UpstreamDatum(
                        'Repository', repo["url"], 'certain')
                if 'web' in repo:
                    yield UpstreamDatum(
                        'Repository-Browse', repo['web'], 'certain')

    # Wild guess:
    if dist_name:
        yield from guess_from_perl_dist_name(path, dist_name)


def guess_from_travis_yml(path, trust_package):
    import ruamel.yaml
    import ruamel.yaml.reader
    with open(path, 'rb') as f:
        try:
            ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
        except ruamel.yaml.reader.ReaderError as e:
            logging.warning('Unable to parse %s: %s', path, e)
            return


def guess_from_meta_yml(path, trust_package):
    """Guess upstream metadata from a META.yml file.

    See http://module-build.sourceforge.net/META-spec-v1.4.html for the
    specification of the format.
    """
    import ruamel.yaml
    import ruamel.yaml.reader
    with open(path, 'rb') as f:
        try:
            data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
        except ruamel.yaml.reader.ReaderError as e:
            logging.warning('Unable to parse %s: %s', path, e)
            return
        if data is None:
            # Empty file?
            return
        if 'name' in data:
            dist_name = data['name']
            yield UpstreamDatum('Name', data['name'], 'certain')
        else:
            dist_name = None
        if data.get('license'):
            yield UpstreamDatum('X-License', data['license'], 'certain')
        if 'version' in data:
            yield UpstreamDatum('X-Version', str(data['version']), 'certain')
        if 'resources' in data:
            resources = data['resources']
            if 'bugtracker' in resources:
                yield UpstreamDatum(
                    'Bug-Database', resources['bugtracker'], 'certain')
            if 'homepage' in resources:
                yield UpstreamDatum(
                    'Homepage', resources['homepage'], 'certain')
            if 'repository' in resources:
                if isinstance(resources['repository'], dict):
                    url = resources['repository'].get('url')
                else:
                    url = resources['repository']
                if url:
                    yield UpstreamDatum(
                        'Repository', url, 'certain')
    # Wild guess:
    if dist_name:
        yield from guess_from_perl_dist_name(path, dist_name)


def guess_from_metainfo(path, trust_package):
    # See https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html
    from xml.etree import ElementTree
    el = ElementTree.parse(path)
    root = el.getroot()
    for child in root:
        if child.tag == 'id':
            yield UpstreamDatum('Name', child.text, 'certain')
        if child.tag == 'project_license':
            yield UpstreamDatum('X-License', child.text, 'certain')
        if child.tag == 'url':
            urltype = child.attrib.get('type')
            if urltype == 'homepage':
                yield UpstreamDatum('Homepage', child.text, 'certain')
            elif urltype == 'bugtracker':
                yield UpstreamDatum('Bug-Database', child.text, 'certain')
        if child.tag == 'description':
            yield UpstreamDatum('X-Description', child.text, 'certain')
        if child.tag == 'summary':
            yield UpstreamDatum('X-Summary', child.text, 'certain')
        if child.tag == 'name':
            yield UpstreamDatum('Name', child.text, 'certain')


def guess_from_doap(path, trust_package):  # noqa: C901
    """Guess upstream metadata from a DOAP file.
    """
    # See https://github.com/ewilderj/doap
    from xml.etree import ElementTree
    el = ElementTree.parse(path)
    root = el.getroot()
    DOAP_NAMESPACE = 'http://usefulinc.com/ns/doap#'
    if root.tag == '{http://www.w3.org/1999/02/22-rdf-syntax-ns#}RDF':
        # If things are wrapped in RDF, unpack.
        [root] = list(root)

    if root.tag != ('{%s}Project' % DOAP_NAMESPACE):
        logging.warning('Doap file does not have DOAP project as root')
        return

    def extract_url(el):
        return el.attrib.get(
            '{http://www.w3.org/1999/02/22-rdf-syntax-ns#}resource')

    def extract_lang(el):
        return el.attrib.get('{http://www.w3.org/XML/1998/namespace}lang')

    screenshots = []

    for child in root:
        if child.tag == ('{%s}name' % DOAP_NAMESPACE) and child.text:
            yield UpstreamDatum('Name', child.text, 'certain')
        elif child.tag == ('{%s}short-name' % DOAP_NAMESPACE) and child.text:
            yield UpstreamDatum('Name', child.text, 'likely')
        elif child.tag == ('{%s}bug-database' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield UpstreamDatum('Bug-Database', url, 'certain')
        elif child.tag == ('{%s}homepage' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield UpstreamDatum('Homepage', url, 'certain')
        elif child.tag == ('{%s}download-page' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield UpstreamDatum('X-Download', url, 'certain')
        elif child.tag == ('{%s}shortdesc' % DOAP_NAMESPACE):
            lang = extract_lang(child)
            if lang in ('en', None):
                yield UpstreamDatum('X-Summary', child.text, 'certain')
        elif child.tag == ('{%s}description' % DOAP_NAMESPACE):
            lang = extract_lang(child)
            if lang in ('en', None):
                yield UpstreamDatum('X-Description', child.text, 'certain')
        elif child.tag == ('{%s}license' % DOAP_NAMESPACE):
            pass  # TODO
        elif child.tag == ('{%s}repository' % DOAP_NAMESPACE):
            for repo in child:
                if repo.tag in (
                        '{%s}SVNRepository' % DOAP_NAMESPACE,
                        '{%s}GitRepository' % DOAP_NAMESPACE):
                    repo_location = repo.find(
                        '{http://usefulinc.com/ns/doap#}location')
                    if repo_location is not None:
                        repo_url = extract_url(repo_location)
                    else:
                        repo_url = None
                    if repo_url:
                        yield UpstreamDatum('Repository', repo_url, 'certain')
                    web_location = repo.find(
                        '{http://usefulinc.com/ns/doap#}browse')
                    if web_location is not None:
                        web_url = extract_url(web_location)
                    else:
                        web_url = None

                    if web_url:
                        yield UpstreamDatum(
                            'Repository-Browse', web_url, 'certain')
        elif child.tag == '{%s}category' % DOAP_NAMESPACE:
            pass
        elif child.tag == '{%s}programming-language' % DOAP_NAMESPACE:
            pass
        elif child.tag == '{%s}os' % DOAP_NAMESPACE:
            pass
        elif child.tag == '{%s}implements' % DOAP_NAMESPACE:
            pass
        elif child.tag == '{https://schema.org/}logo':
            pass
        elif child.tag == '{https://schema.org/}screenshot':
            url = extract_url(child)
            if url:
                screenshots.append(url)
        elif child.tag == '{%s}wiki' % DOAP_NAMESPACE:
            url = extract_url(child)
            if url:
                yield UpstreamDatum('X-Wiki', url, 'certain')
        elif child.tag == '{%s}maintainer' % DOAP_NAMESPACE:
            for person in child:
                if person.tag != '{http://xmlns.com/foaf/0.1/}Person':
                    continue
                name = person.find('{http://xmlns.com/foaf/0.1/}name').text
                email_tag = person.find('{http://xmlns.com/foaf/0.1/}mbox')
                maintainer = Person(
                    name, email_tag.text if email_tag else None)
                yield UpstreamDatum('X-Maintainer', maintainer, 'certain')
        else:
            logging.warning('Unknown tag %s in DOAP file', child.tag)


def guess_from_cabal_lines(lines):
    # TODO(jelmer): Perhaps use a standard cabal parser in Python?
    # The current parser is not really correct, but good enough for our needs.
    # https://www.haskell.org/cabal/release/cabal-1.10.1.0/doc/users-guide/
    repo_url = None
    repo_branch = None
    repo_subpath = None

    section = None
    for line in lines:
        if line.lstrip().startswith('--'):
            # Comment
            continue
        if not line.strip():
            section = None
            continue
        try:
            (field, value) = line.split(':', 1)
        except ValueError:
            if not line.startswith(' '):
                section = line.strip().lower()
            continue
        # The case of field names is not sigificant
        field = field.lower()
        value = value.strip()
        if not field.startswith(' '):
            if field == 'homepage':
                yield 'Homepage', value
            if field == 'bug-reports':
                yield 'Bug-Database', value
            if field == 'name':
                yield 'Name', value
            if field == 'maintainer':
                yield 'X-Maintainer', Person.from_string(value)
            if field == 'copyright':
                yield 'X-Copyright', value
            if field == 'license':
                yield 'X-License', value
        else:
            field = field.strip()
            if section == 'source-repository head':
                if field == 'location':
                    repo_url = value
                if field == 'branch':
                    repo_branch = value
                if field == 'subdir':
                    repo_subpath = value
    if repo_url:
        yield (
            'Repository',
            unsplit_vcs_url(repo_url, repo_branch, repo_subpath))


def guess_from_cabal(path, trust_package=False):  # noqa: C901
    with open(path, 'r', encoding='utf-8') as f:
        for name, value in guess_from_cabal_lines(f):
            yield UpstreamDatum(name, value, 'certain', origin=path)


def is_email_address(value: str) -> bool:
    return '@' in value or ' (at) ' in value


def guess_from_configure(path, trust_package=False):
    if os.path.isdir(path):
        return
    with open(path, 'rb') as f:
        for line in f:
            if b'=' not in line:
                continue
            (key, value) = line.strip().split(b'=', 1)
            if b' ' in key:
                continue
            if b'$' in value:
                continue
            value = value.strip()
            if value.startswith(b"'") and value.endswith(b"'"):
                value = value[1:-1]
            if not value:
                continue
            if key == b'PACKAGE_NAME':
                yield UpstreamDatum(
                    'Name', value.decode(), 'certain', './configure')
            elif key == b'PACKAGE_VERSION':
                yield UpstreamDatum(
                    'X-Version', value.decode(), 'certain', './configure')
            elif key == b'PACKAGE_BUGREPORT':
                if value in (b'BUG-REPORT-ADDRESS', ):
                    certainty = 'invalid'
                elif (is_email_address(value.decode()) and
                        not value.endswith(b'gnu.org')):
                    # Downgrade the trustworthiness of this field for most
                    # upstreams if it contains an e-mail address. Most
                    # upstreams seem to just set this to some random address,
                    # and then forget about it.
                    certainty = 'possible'
                else:
                    parsed_url = urlparse(value.decode())
                    if parsed_url.path.strip('/'):
                        certainty = 'certain'
                    else:
                        # It seems unlikely that the bug submit URL lives at
                        # the root.
                        certainty = 'possible'
                if certainty != 'invalid':
                    yield UpstreamDatum(
                        'Bug-Submit', value.decode(), certainty, './configure')
            elif key == b'PACKAGE_URL':
                yield UpstreamDatum(
                    'Homepage', value.decode(), 'certain', './configure')


def guess_from_r_description(path, trust_package=False):
    import textwrap
    # See https://r-pkgs.org/description.html
    with open(path, 'rb') as f:
        # TODO(jelmer): use rfc822 instead?
        from debian.deb822 import Deb822

        description = Deb822(f)
        if 'Package' in description:
            yield UpstreamDatum('Name', description['Package'], 'certain')
        if 'Repository' in description:
            yield UpstreamDatum(
                'Archive', description['Repository'], 'certain')
        if 'BugReports' in description:
            yield UpstreamDatum(
                'Bug-Database', description['BugReports'], 'certain')
        if 'Version' in description:
            yield UpstreamDatum('X-Version', description['Version'], 'certain')
        if 'License' in description:
            yield UpstreamDatum('X-License', description['License'], 'certain')
        if 'Title' in description:
            yield UpstreamDatum('X-Summary', description['Title'], 'certain')
        if 'Description' in description:
            lines = description['Description'].splitlines(True)
            reflowed = lines[0] + textwrap.dedent(''.join(lines[1:]))
            yield UpstreamDatum('X-Description', reflowed, 'certain')
        if 'Maintainer' in description:
            yield UpstreamDatum(
                'X-Maintainer', Person.from_string(description['Maintainer']), 'certain')
        if 'URL' in description:
            entries = [entry.strip()
                       for entry in re.split('[\n,]', description['URL'])]
            urls = []
            for entry in entries:
                m = re.match('([^ ]+) \\((.*)\\)', entry)
                if m:
                    url = m.group(1)
                    label = m.group(2)
                else:
                    url = entry
                    label = None
                urls.append((label, url))
            if len(urls) == 1:
                yield UpstreamDatum('Homepage', urls[0][1], 'possible')
            for label, url in urls:
                parsed_url = urlparse(url)
                if parsed_url.hostname == 'bioconductor.org':
                    yield UpstreamDatum('Archive', 'Bioconductor', 'confident')
                if label and label.lower() in ('devel', 'repository'):
                    yield UpstreamDatum('Repository', sanitize_vcs_url(url), 'certain')
                elif label and label.lower() in ('homepage', ):
                    yield UpstreamDatum('Homepage', url, 'certain')
                else:
                    repo_url = guess_repo_from_url(url)
                    if repo_url:
                        yield UpstreamDatum('Repository', sanitize_vcs_url(repo_url), 'certain')


def guess_from_environment():
    try:
        yield UpstreamDatum(
            'Repository', os.environ['UPSTREAM_BRANCH_URL'], 'certain')
    except KeyError:
        pass


def guess_from_path(path):
    basename = os.path.basename(os.path.abspath(path))
    m = re.fullmatch('(.*)-([0-9.]+)', basename)
    if m:
        yield UpstreamDatum('Name', m.group(1), 'possible')
        yield UpstreamDatum('X-Version', m.group(2), 'possible')
    else:
        yield UpstreamDatum('Name', basename, 'possible')


def guess_from_cargo(path, trust_package):
    # see https://doc.rust-lang.org/cargo/reference/manifest.html
    try:
        from tomlkit import loads
        from tomlkit.exceptions import ParseError
    except ImportError:
        return
    try:
        with open(path, 'r') as f:
            cargo = loads(f.read())
    except FileNotFoundError:
        return
    except ParseError as e:
        logging.warning('Error parsing toml file %s: %s', path, e)
        return
    try:
        package = cargo['package']
    except KeyError:
        pass
    else:
        if 'name' in package:
            yield UpstreamDatum('Name', str(package['name']), 'certain')
        if 'description' in package:
            yield UpstreamDatum('X-Summary', str(package['description']), 'certain')
        if 'homepage' in package:
            yield UpstreamDatum('Homepage', str(package['homepage']), 'certain')
        if 'license' in package:
            yield UpstreamDatum('X-License', str(package['license']), 'certain')
        if 'repository' in package:
            yield UpstreamDatum('Repository', str(package['repository']), 'certain')
        if 'version' in package:
            yield UpstreamDatum('X-Version', str(package['version']), 'confident')


def guess_from_pyproject_toml(path, trust_package):
    try:
        from tomlkit import loads
        from tomlkit.exceptions import ParseError
    except ImportError:
        return
    try:
        with open(path, 'r') as f:
            pyproject = loads(f.read())
    except FileNotFoundError:
        return
    except ParseError as e:
        logging.warning('Error parsing toml file %s: %s', path, e)
        return
    if 'poetry' in pyproject.get('tool', []):
        poetry = pyproject['tool']['poetry']
        if 'version' in poetry:
            yield UpstreamDatum('X-Version', str(poetry['version']), 'certain')
        if 'description' in poetry:
            yield UpstreamDatum('X-Summary', str(poetry['description']), 'certain')
        if 'license' in poetry:
            yield UpstreamDatum('X-License', str(poetry['license']), 'certain')
        if 'repository' in poetry:
            yield UpstreamDatum('Repository', str(poetry['repository']), 'certain')
        if 'name' in poetry:
            yield UpstreamDatum('Name', str(poetry['name']), 'certain')


def guess_from_pom_xml(path, trust_package=False):  # noqa: C901
    # Documentation: https://maven.apache.org/pom.html

    import xml.etree.ElementTree as ET
    try:
        root = xmlparse_simplify_namespaces(path, [
            'http://maven.apache.org/POM/4.0.0'])
    except ET.ParseError as e:
        logging.warning('Unable to parse package.xml: %s', e)
        return
    assert root.tag == 'project', 'root tag is %r' % root.tag
    name_tag = root.find('name')
    if name_tag is not None and '$' not in name_tag.text:
        yield UpstreamDatum('Name', name_tag.text, 'certain')
    else:
        artifact_id_tag = root.find('artifactId')
        if artifact_id_tag is not None:
            yield UpstreamDatum('Name', artifact_id_tag.text, 'possible')
    description_tag = root.find('description')
    if description_tag is not None and description_tag.text:
        yield UpstreamDatum('X-Summary', description_tag.text, 'certain')
    version_tag = root.find('version')
    if version_tag is not None and '$' not in version_tag.text:
        yield UpstreamDatum('X-Version', version_tag.text, 'certain')
    licenses_tag = root.find('licenses')
    if licenses_tag is not None:
        licenses = []
        for license_tag in licenses_tag.findall('license'):
            name_tag = license_tag.find('name')
            if name_tag is not None:
                licenses.append(name_tag.text)
    for scm_tag in root.findall('scm'):
        url_tag = scm_tag.find('url')
        if url_tag is not None:
            if (url_tag.text.startswith('scm:') and
                    url_tag.text.count(':') >= 3):
                url = url_tag.text.split(':', 2)[2]
            else:
                url = url_tag.text
            if plausible_vcs_browse_url(url):
                yield UpstreamDatum('Repository-Browse', url, 'certain')
        connection_tag = scm_tag.find('connection')
        if connection_tag is not None:
            connection = connection_tag.text
            try:
                (scm, provider, provider_specific) = connection.split(':', 2)
            except ValueError:
                logging.warning(
                    'Invalid format for SCM connection: %s', connection)
                continue
            if scm != 'scm':
                logging.warning(
                    'SCM connection does not start with scm: prefix: %s',
                    connection)
                continue
            yield UpstreamDatum(
                'Repository', provider_specific, 'certain')
    for issue_mgmt_tag in root.findall('issueManagement'):
        url_tag = issue_mgmt_tag.find('url')
        if url_tag is not None:
            yield UpstreamDatum('Bug-Database', url_tag.text, 'certain')
    url_tag = root.find('url')
    if url_tag:
        if not url_tag.text.startswith('scm:'):
            # Yeah, uh, not a URL.
            pass
        else:
            yield UpstreamDatum('Homepage', url_tag.text, 'certain')


def guess_from_git_config(path, trust_package=False):
    # See https://git-scm.com/docs/git-config
    from dulwich.config import ConfigFile

    cfg = ConfigFile.from_path(path)
    # If there's a remote named upstream, that's a plausible source..
    try:
        urlb = cfg.get((b'remote', b'upstream'), b'url')
    except KeyError:
        pass
    else:
        url = urlb.decode('utf-8')
        if not url.startswith('../'):
            yield UpstreamDatum('Repository', url, 'likely')

    # It's less likely that origin is correct, but let's try anyway
    # (with a lower certainty)
    # Either way, it's probably incorrect if this is a packaging
    # repository.
    if not os.path.exists(
            os.path.join(os.path.dirname(path), '..', 'debian')):
        try:
            urlb = cfg.get((b'remote', b'origin'), b'url')
        except KeyError:
            pass
        else:
            url = urlb.decode('utf-8')
            if not url.startswith('../'):
                yield UpstreamDatum('Repository', url, 'possible')


def guess_from_get_orig_source(path, trust_package=False):
    with open(path, 'rb') as f:
        for line in f:
            if line.startswith(b'git clone'):
                url = url_from_git_clone_command(line)
                if url:
                    yield UpstreamDatum('Repository', url, 'likely')


# https://docs.github.com/en/free-pro-team@latest/github/\
# managing-security-vulnerabilities/adding-a-security-policy-to-your-repository
def guess_from_security_md(path, trust_package=False):
    if path.startswith('./'):
        path = path[2:]
    # TODO(jelmer): scan SECURITY.md for email addresses/URLs with instructions
    yield UpstreamDatum('X-Security-MD', path, 'certain')


def guess_from_go_mod(path, trust_package=False):
    # See https://golang.org/doc/modules/gomod-ref
    with open(path, 'rb') as f:
        for line in f:
            if line.startswith(b'module '):
                modname = line.strip().split(b' ', 1)[1]
                yield UpstreamDatum('Name', modname.decode('utf-8'), 'certain')


def guess_from_gemspec(path, trust_package=False):
    # TODO(jelmer): use a proper ruby wrapper instead?
    with open(path, 'r') as f:
        for line in f:
            if line.startswith('#'):
                continue
            if not line.strip():
                continue
            if line in ('Gem::Specification.new do |s|\n', 'end\n'):
                continue
            if line.startswith('  s.'):
                try:
                    (key, rawval) = line[4:].split('=', 1)
                except ValueError:
                    continue
                key = key.strip()
                rawval = rawval.strip()
                if rawval.startswith('"') and rawval.endswith('".freeze'):
                    val = rawval[1:-len('".freeze')]
                elif rawval.startswith('"') and rawval.endswith('"'):
                    val = rawval[1:-1]
                else:
                    continue
                if key == "name":
                    yield UpstreamDatum('Name', val, 'certain')
                elif key == 'version':
                    yield UpstreamDatum('X-Version', val, 'certain')
                elif key == 'homepage':
                    yield UpstreamDatum('Homepage', val, 'certain')
                elif key == 'summary':
                    yield UpstreamDatum('X-Summary', val, 'certain')
                elif key == 'description':
                    yield UpstreamDatum('X-Description', val, 'certain')
            else:
                logging.debug(
                    'ignoring unparseable line in %s: %r',
                    path, line)


def guess_from_makefile_pl(path, trust_package=False):
    dist_name = None
    with open(path, 'rb') as f:
        for line in f:
            m = re.fullmatch(br"name '([^'\"]+)';$", line.rstrip())
            if m:
                dist_name = m.group(1).decode()
                yield UpstreamDatum('Name', dist_name, 'confident')
            m = re.fullmatch(br"repository '([^'\"]+)';$", line.rstrip())
            if m:
                yield UpstreamDatum('Repository', m.group(1).decode(), 'confident')

    if dist_name:
        yield from guess_from_perl_dist_name(path, dist_name)


def guess_from_wscript(path, trust_package=False):
    with open(path, 'rb') as f:
        for line in f:
            m = re.fullmatch(b'APPNAME = [\'"](.*)[\'"]', line.rstrip(b'\n'))
            if m:
                yield UpstreamDatum('Name', m.group(1).decode(), 'confident')
            m = re.fullmatch(b'VERSION = [\'"](.*)[\'"]', line.rstrip(b'\n'))
            if m:
                yield UpstreamDatum('X-Version', m.group(1).decode(), 'confident')


def guess_from_authors(path, trust_package=False):
    authors = []
    with open(path, 'rb') as f:
        for line in f:
            m = line.strip().decode('utf-8', 'surrogateescape')
            if not m:
                continue
            if not m[0].isalpha():
                continue
            if m.startswith('*') or m.startswith('-'):
                m = m[1:].strip()
            if len(m) < 3:
                continue
            if m.endswith('.'):
                continue
            if '<' in m or m.count(' ') < 5:
                authors.append(Person.from_string(m))
    yield UpstreamDatum('X-Authors', authors, 'certain')


def _get_guessers(path, trust_package=False):  # noqa: C901
    CANDIDATES = [
        ('debian/watch', guess_from_debian_watch),
        ('debian/control', guess_from_debian_control),
        ('debian/changelog', guess_from_debian_changelog),
        ('debian/rules', guess_from_debian_rules),
        ('PKG-INFO', guess_from_pkg_info),
        ('package.json', guess_from_package_json),
        ('composer.json', guess_from_composer_json),
        ('package.xml', guess_from_package_xml),
        ('dist.ini', guess_from_dist_ini),
        ('debian/copyright', guess_from_debian_copyright),
        ('META.json', guess_from_meta_json),
        ('MYMETA.json', guess_from_meta_json),
        ('META.yml', guess_from_meta_yml),
        ('MYMETA.yml', guess_from_meta_yml),
        ('configure', guess_from_configure),
        ('DESCRIPTION', guess_from_r_description),
        ('Cargo.toml', guess_from_cargo),
        ('pom.xml', guess_from_pom_xml),
        ('.git/config', guess_from_git_config),
        ('debian/get-orig-source.sh', guess_from_get_orig_source),
        ('SECURITY.md', guess_from_security_md),
        ('.github/SECURITY.md', guess_from_security_md),
        ('docs/SECURITY.md', guess_from_security_md),
        ('pyproject.toml', guess_from_pyproject_toml),
        ('setup.cfg', guess_from_setup_cfg),
        ('go.mod', guess_from_go_mod),
        ('Makefile.PL', guess_from_makefile_pl),
        ('wscript', guess_from_wscript),
        ('AUTHORS', guess_from_authors),
        ]

    # Search for something Python-y
    found_pkg_info = os.path.exists(os.path.join(path, 'PKG-INFO'))
    for entry in os.scandir(path):
        if entry.name.endswith('.egg-info'):
            CANDIDATES.append(
                (os.path.join(entry.name, 'PKG-INFO'), guess_from_pkg_info))
            found_pkg_info = True
        if entry.name.endswith('.dist-info'):
            CANDIDATES.append(
                (os.path.join(entry.name, 'METADATA'), guess_from_pkg_info))
            found_pkg_info = True
    if not found_pkg_info and os.path.exists(os.path.join(path, 'setup.py')):
        CANDIDATES.append(('setup.py', guess_from_setup_py))

    for entry in os.scandir(path):
        if entry.name.endswith('.gemspec'):
            CANDIDATES.append((entry.name, guess_from_gemspec))

    # TODO(jelmer): Perhaps scan all directories if no other primary project
    # information file has been found?
    for entry in os.scandir(path):
        if entry.is_dir():
            subpath = os.path.join(entry.path, 'DESCRIPTION')
            if os.path.exists(subpath):
                CANDIDATES.append(
                    (os.path.join(entry.name, 'DESCRIPTION'),
                     guess_from_r_description))

    doap_filenames = [
        n for n in os.listdir(path)
        if n.endswith('.doap') or
        (n.endswith('.xml') and n.startswith('doap_XML_'))]
    if doap_filenames:
        if len(doap_filenames) == 1:
            CANDIDATES.append((doap_filenames[0], guess_from_doap))
        else:
            logging.warning(
                'More than one doap filename, ignoring all: %r',
                doap_filenames)

    metainfo_filenames = [
        n for n in os.listdir(path)
        if n.endswith('.metainfo.xml')]
    if metainfo_filenames:
        if len(metainfo_filenames) == 1:
            CANDIDATES.append((metainfo_filenames[0], guess_from_metainfo))
        else:
            logging.warning(
                'More than one metainfo filename, ignoring all: %r',
                metainfo_filenames)

    cabal_filenames = [n for n in os.listdir(path) if n.endswith('.cabal')]
    if cabal_filenames:
        if len(cabal_filenames) == 1:
            CANDIDATES.append((cabal_filenames[0], guess_from_cabal))
        else:
            logging.warning(
                'More than one cabal filename, ignoring all: %r',
                cabal_filenames)

    readme_filenames = [
        n for n in os.listdir(path)
        if any([n.startswith(p)
                for p in ['readme', 'ReadMe', 'Readme', 'README', 'HACKING', 'CONTRIBUTING']])
        and os.path.splitext(n)[1] not in ('.html', '.pdf', '.xml')
        and not n.endswith('~')]
    CANDIDATES.extend([(n, guess_from_readme) for n in readme_filenames])

    try:
        debian_patches = [
            os.path.join('debian', 'patches', n)
            for n in os.listdir('debian/patches')
            if os.path.isfile(os.path.join('debian/patches', n))]
    except FileNotFoundError:
        pass
    else:
        CANDIDATES.extend(
            [(p, guess_from_debian_patch) for p in debian_patches])

    yield 'environment', guess_from_environment()
    yield 'path', guess_from_path(path)

    for relpath, guesser in CANDIDATES:
        abspath = os.path.join(path, relpath)
        if not os.path.exists(abspath):
            continue
        yield relpath, guesser(abspath, trust_package=trust_package)


def guess_upstream_metadata_items(
        path: str, trust_package: bool = False,
        minimum_certainty: Optional[str] = None
        ) -> Iterable[UpstreamDatum]:
    """Guess upstream metadata items, in no particular order.

    Args:
      path: Path to the package
      trust: Whether to trust the package contents and i.e. run
      executables in it
    Yields:
      UpstreamDatum
    """
    for entry in guess_upstream_info(path, trust_package=trust_package):
        if isinstance(entry, UpstreamDatum):
            if certainty_sufficient(entry.certainty, minimum_certainty):
                yield entry


def guess_upstream_info(
        path: str, trust_package: bool = False) -> Iterable[UpstreamDatum]:
    guessers = _get_guessers(path, trust_package=trust_package)
    for name, guesser in guessers:
        for entry in guesser:
            if entry.origin is None:
                entry.origin = name
            yield entry


def get_upstream_info(path, trust_package=False, net_access=False,
                      consult_external_directory=False, check=False):
    metadata_items = []
    for entry in guess_upstream_info(path, trust_package=trust_package):
        if isinstance(entry, UpstreamDatum):
            metadata_items.append(entry)
    metadata = summarize_upstream_metadata(
        metadata_items, path, net_access=net_access,
        consult_external_directory=consult_external_directory,
        check=check)
    return metadata


def summarize_upstream_metadata(
        metadata_items, path, net_access=False,
        consult_external_directory=False, check=False):
    """Summarize the upstream metadata into a dictionary.

    Args:
      metadata_items: Iterator over metadata items
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
      net_access: Whether to allow net access
      consult_external_directory: Whether to pull in data
        from external (user-maintained) directories.
    """
    upstream_metadata = {}
    update_from_guesses(
        upstream_metadata,
        filter_bad_guesses(metadata_items))

    extend_upstream_metadata(
        upstream_metadata, path, net_access=net_access,
        consult_external_directory=consult_external_directory)

    if check:
        check_upstream_metadata(upstream_metadata)

    fix_upstream_metadata(upstream_metadata)

    return {k: v.value for (k, v) in upstream_metadata.items()}


def guess_upstream_metadata(
        path, trust_package=False, net_access=False,
        consult_external_directory=False, check=False):
    """Guess the upstream metadata dictionary.

    Args:
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
      net_access: Whether to allow net access
      consult_external_directory: Whether to pull in data
        from external (user-maintained) directories.
    """
    metadata_items = guess_upstream_metadata_items(
        path, trust_package=trust_package)
    return summarize_upstream_metadata(
        metadata_items, path, net_access=net_access,
        consult_external_directory=consult_external_directory, check=check)


def _possible_fields_missing(upstream_metadata, fields, field_certainty):
    for field in fields:
        if field not in upstream_metadata:
            return True
        if upstream_metadata[field].certainty != 'certain':
            return True
    else:
        return False


def _sf_git_extract_url(page):
    try:
        from bs4 import BeautifulSoup
    except ModuleNotFoundError:
        logging.warning(
            'Not scanning sourceforge page, since python3-bs4 is missing')
        return None
    bs = BeautifulSoup(page, features='lxml')
    el = bs.find(id='access_url')
    if not el:
        return None
    value = el.get('value')
    if value is None:
        return None
    access_command = value.split(' ')
    if access_command[:2] != ['git', 'clone']:
        return None
    return access_command[2]


def guess_from_sf(sf_project):
    try:
        data = get_sf_metadata(sf_project)
    except socket.timeout:
        logging.warning(
            'timeout contacting launchpad, ignoring: %s',
            sf_project)
        return
    if data.get('name'):
        yield 'Name', data['name']
    if data.get('external_homepage'):
        yield 'Homepage', data['external_homepage']
    if data.get('preferred_support_url'):
        if verify_bug_database_url(data['preferred_support_url']):
            yield 'Bug-Database', data['preferred_support_url']
    # In theory there are screenshots linked from the sourceforge project that
    # we can use, but if there are multiple "subprojects" then it will be
    # unclear which one they belong to.
    # TODO(jelmer): What about cvs and bzr?
    VCS_NAMES = ['hg', 'git', 'svn']
    vcs_tools = [
        (tool['name'], tool['url'])
        for tool in data.get('tools', []) if tool['name'] in VCS_NAMES]
    if len(vcs_tools) == 1:
        (kind, url) = vcs_tools[0]
        if kind == 'git':
            url = urljoin('https://sourceforge.net/', url)
            headers = {'User-Agent': USER_AGENT, 'Accept': 'text/html'}
            http_contents = urlopen(
                Request(url, headers=headers),
                timeout=DEFAULT_URLLIB_TIMEOUT).read()
            url = _sf_git_extract_url(http_contents)
        elif kind == 'svn':
            url = urljoin('https://svn.code.sf.net/', url)
        elif kind == 'hg':
            url = urljoin('https://hg.code.sf.net/', url)
        else:
            raise KeyError(kind)
        if url is not None:
            yield 'Repository', url


def guess_from_repology(repology_project):
    try:
        metadata = get_repology_metadata(repology_project)
    except socket.timeout:
        logging.warning(
            'timeout contacting repology, ignoring: %s', repology_project)
        return

    fields = {}

    def _add_field(name, value, add):
        fields.setdefault(name, {})
        fields[name].setdefault(value, 0)
        fields[name][value] += add

    for entry in metadata:
        if entry.get('status') == 'outdated':
            score = 1
        else:
            score = 10

        if 'www' in entry:
            for www in entry['www']:
                _add_field('Homepage', www, score)

        if 'licenses' in entry:
            for license in entry['licenses']:
                _add_field('X-License', license, score)

        if 'summary' in entry:
            _add_field('X-Summary', entry['summary'], score)

        if 'downloads' in entry:
            for download in entry['downloads']:
                _add_field('X-Download', download, score)

    for field, scores in fields.items():
        yield field, max(scores, key=operator.itemgetter(1))


def extend_from_external_guesser(
        upstream_metadata, guesser_certainty, guesser_fields, guesser):
    if not _possible_fields_missing(
            upstream_metadata, guesser_fields, guesser_certainty):
        return

    update_from_guesses(
        upstream_metadata,
        [UpstreamDatum(key, value, guesser_certainty)
         for (key, value) in guesser])


def extend_from_repology(upstream_metadata, minimum_certainty, source_package):
    # The set of fields that sf can possibly provide:
    repology_fields = ['Homepage', 'X-License', 'X-Summary', 'X-Download']
    certainty = 'confident'

    if certainty_sufficient(certainty, minimum_certainty):
        # Don't bother talking to repology if we're not
        # speculating.
        return

    return extend_from_external_guesser(
        upstream_metadata, certainty, repology_fields,
        guess_from_repology(source_package))


class NoSuchHackagePackage(Exception):

    def __init__(self, package):
        self.package = package


def guess_from_hackage(hackage_package):
    http_url = 'http://hackage.haskell.org/package/%s/%s.cabal' % (
        hackage_package, hackage_package)
    headers = {'User-Agent': USER_AGENT}
    try:
        http_contents = urlopen(
            Request(http_url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT).read()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            raise NoSuchHackagePackage(hackage_package)
        raise
    return guess_from_cabal_lines(
        http_contents.decode('utf-8', 'surrogateescape').splitlines(True))


def extend_from_hackage(upstream_metadata, hackage_package):
    # The set of fields that sf can possibly provide:
    hackage_fields = [
        'Homepage', 'Name', 'Repository', 'X-Maintainer', 'X-Copyright',
        'X-License', 'Bug-Database']
    hackage_certainty = upstream_metadata['Archive'].certainty

    return extend_from_external_guesser(
        upstream_metadata, hackage_certainty, hackage_fields,
        guess_from_hackage(hackage_package))


def guess_from_crates_io(crate):
    data = _load_json_url('https://crates.io/api/v1/crates/%s' % crate)
    crate_data = data['crate']
    yield 'Name', crate_data['name']
    if crate_data.get('homepage'):
        yield 'Homepage', crate_data['homepage']
    if crate_data.get('repository'):
        yield 'Repository', crate_data['repository']
    if crate_data.get('newest_version'):
        yield 'X-Version', crate_data['newest_version']
    if crate_data.get('description'):
        yield 'X-Summary', crate_data['description']


class NoSuchCrate(Exception):

    def __init__(self, crate):
        self.crate = crate


def extend_from_crates_io(upstream_metadata, crate):
    # The set of fields that crates.io can possibly provide:
    crates_io_fields = [
        'Homepage', 'Name', 'Repository']
    crates_io_certainty = upstream_metadata['Archive'].certainty

    return extend_from_external_guesser(
        upstream_metadata, crates_io_certainty, crates_io_fields,
        guess_from_crates_io(crate))


def extend_from_sf(upstream_metadata, sf_project):
    # The set of fields that sf can possibly provide:
    sf_fields = ['Homepage', 'Name', 'Repository']
    sf_certainty = upstream_metadata['Archive'].certainty

    return extend_from_external_guesser(
        upstream_metadata, sf_certainty, sf_fields,
        guess_from_sf(sf_project))


def extend_from_pecl(upstream_metadata, pecl_url, certainty):
    pecl_fields = ['Homepage', 'Repository', 'Bug-Database']

    return extend_from_external_guesser(
        upstream_metadata, certainty, pecl_fields,
        guess_from_pecl_url(pecl_url))


def extend_from_lp(upstream_metadata, minimum_certainty, package,
                   distribution=None, suite=None):
    # The set of fields that Launchpad can possibly provide:
    lp_fields = ['Homepage', 'Repository', 'Name']
    lp_certainty = 'possible'

    if certainty_sufficient(lp_certainty, minimum_certainty):
        # Don't bother talking to launchpad if we're not
        # speculating.
        return

    extend_from_external_guesser(
        upstream_metadata, lp_certainty, lp_fields, guess_from_launchpad(
             package, distribution=distribution, suite=suite))


def extend_from_aur(upstream_metadata, minimum_certainty, package):
    # The set of fields that AUR can possibly provide:
    aur_fields = ['Homepage', 'Repository']
    aur_certainty = 'possible'

    if certainty_sufficient(aur_certainty, minimum_certainty):
        # Don't bother talking to AUR if we're not speculating.
        return

    extend_from_external_guesser(
        upstream_metadata, aur_certainty, aur_fields, guess_from_aur(package))


def extract_sf_project_name(url):
    if isinstance(url, list):
        return None
    m = re.fullmatch('https?://(.*).(sf|sourceforge).net/?', url)
    if m:
        return m.group(1)
    m = re.match('https://sourceforge.net/(projects|p)/([^/]+)', url)
    if m:
        return m.group(2)


def repo_url_from_merge_request_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) > 2 and path_elements[2] == 'issues':
            return urlunparse(
                ('https', 'github.com', '/'.join(path_elements[:3]),
                 None, None, None))
    if is_gitlab_site(parsed_url.netloc):
        path_elements = parsed_url.path.strip('/').split('/')
        if (len(path_elements) > 2 and
                path_elements[-2] == 'merge_requests' and
                path_elements[-1].isdigit()):
            return urlunparse(
                ('https', parsed_url.netloc, '/'.join(path_elements[:-2]),
                 None, None, None))


def bug_database_from_issue_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) > 2 and path_elements[2] == 'issues':
            return urlunparse(
                ('https', 'github.com', '/'.join(path_elements[:3]),
                 None, None, None))
    if is_gitlab_site(parsed_url.netloc):
        path_elements = parsed_url.path.strip('/').split('/')
        if (len(path_elements) > 2 and
                path_elements[-2] == 'issues' and
                path_elements[-1].isdigit()):
            return urlunparse(
                ('https', parsed_url.netloc, '/'.join(path_elements[:-2]),
                 None, None, None))


def guess_bug_database_url_from_repo_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path = '/'.join(parsed_url.path.split('/')[:3])
        if path.endswith('.git'):
            path = path[:-4]
        path = path + '/issues'
        return urlunparse(
            ('https', 'github.com', path,
             None, None, None))
    if is_gitlab_site(parsed_url.hostname):
        path = '/'.join(parsed_url.path.split('/')[:3])
        if path.endswith('.git'):
            path = path[:-4]
        path = path + '/issues'
        return urlunparse(
            ('https', parsed_url.hostname, path,
             None, None, None))
    return None


def bug_database_url_from_bug_submit_url(url):
    parsed_url = urlparse(url)
    path_elements = parsed_url.path.strip('/').split('/')
    if parsed_url.netloc == 'github.com':
        if len(path_elements) not in (3, 4):
            return None
        if path_elements[2] != 'issues':
            return None
        return urlunparse(
            ('https', 'github.com', '/'.join(path_elements[:3]),
             None, None, None))
    if parsed_url.netloc == 'bugs.launchpad.net':
        if len(path_elements) >= 1:
            return urlunparse(
                parsed_url._replace(path='/%s' % path_elements[0]))
    if is_gitlab_site(parsed_url.netloc):
        if len(path_elements) < 2:
            return None
        if path_elements[-2] != 'issues':
            return None
        if path_elements[-1] == 'new':
            path_elements.pop(-1)
        return urlunparse(
            parsed_url._replace(path='/'.join(path_elements)))
    if parsed_url.hostname == 'sourceforge.net':
        if len(path_elements) < 3:
            return None
        if path_elements[0] != 'p' or path_elements[2] != 'bugs':
            return None
        if len(path_elements) > 3:
            path_elements.pop(-1)
        return urlunparse(
            parsed_url._replace(path='/'.join(path_elements)))
    return None


def bug_submit_url_from_bug_database_url(url):
    parsed_url = urlparse(url)
    path_elements = parsed_url.path.strip('/').split('/')
    if parsed_url.netloc == 'github.com':
        if len(path_elements) != 3:
            return None
        if path_elements[2] != 'issues':
            return None
        return urlunparse(
            ('https', 'github.com', parsed_url.path + '/new',
             None, None, None))
    if parsed_url.netloc == 'bugs.launchpad.net':
        if len(path_elements) == 1:
            return urlunparse(
                parsed_url._replace(path=parsed_url.path+'/+filebug'))
    if is_gitlab_site(parsed_url.netloc):
        if len(path_elements) < 2:
            return None
        if path_elements[-1] != 'issues':
            return None
        return urlunparse(
            parsed_url._replace(path=parsed_url.path.rstrip('/')+'/new'))
    return None


def verify_bug_database_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) < 3 or path_elements[2] != 'issues':
            return False
        api_url = 'https://api.github.com/repos/%s/%s' % (
            path_elements[0], path_elements[1])
        try:
            data = _load_json_url(api_url)
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return False
            if e.code == 403:
                # Probably rate limited
                logging.warning(
                    'Unable to verify bug database URL %s: %s',
                    url, e.reason)
                return None
            raise
        return data['has_issues'] and not data.get('archived', False)
    if is_gitlab_site(parsed_url.netloc):
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) < 3 or path_elements[-1] != 'issues':
            return False
        api_url = 'https://%s/api/v4/projects/%s/issues' % (
            parsed_url.netloc, quote('/'.join(path_elements[:-1]), safe=''))
        try:
            data = _load_json_url(api_url)
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return False
            raise
        return len(data) > 0
    return None


def verify_bug_submit_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com' or is_gitlab_site(parsed_url.netloc):
        path = '/'.join(parsed_url.path.strip('/').split('/')[:-1])
        return verify_bug_database_url(
            urlunparse(parsed_url._replace(path=path)))
    return None


def _extrapolate_repository_from_homepage(upstream_metadata, net_access):
    repo = guess_repo_from_url(
            upstream_metadata['Homepage'].value, net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            min_certainty(['likely', upstream_metadata['Homepage'].certainty]))


def _extrapolate_repository_from_download(upstream_metadata, net_access):
    repo = guess_repo_from_url(
            upstream_metadata['X-Download'].value, net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            min_certainty(
                ['likely', upstream_metadata['X-Download'].certainty]))


def _extrapolate_repository_from_bug_db(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Bug-Database'].value, net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            min_certainty(
                ['likely', upstream_metadata['Bug-Database'].certainty]))


def _extrapolate_name_from_repository(upstream_metadata, net_access):
    repo = guess_repo_from_url(
            upstream_metadata['Repository'].value, net_access=net_access)
    if repo:
        parsed = urlparse(repo)
        name = parsed.path.split('/')[-1]
        if name.endswith('.git'):
            name = name[:-4]
        if name:
            return UpstreamDatum('Name', name, min_certainty(
                    ['likely', upstream_metadata['Repository'].certainty]))


def _extrapolate_repository_browse_from_repository(
        upstream_metadata, net_access):
    browse_url = browse_url_from_repo_url(
            upstream_metadata['Repository'].value)
    if browse_url:
        return UpstreamDatum(
            'Repository-Browse', browse_url,
            upstream_metadata['Repository'].certainty)


def _extrapolate_repository_from_repository_browse(
        upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Repository-Browse'].value,
        net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            upstream_metadata['Repository-Browse'].certainty)


def _extrapolate_bug_database_from_repository(
        upstream_metadata, net_access):
    repo_url = upstream_metadata['Repository'].value
    if not isinstance(repo_url, str):
        return
    bug_db_url = guess_bug_database_url_from_repo_url(repo_url)
    if bug_db_url:
        return UpstreamDatum(
            'Bug-Database', bug_db_url,
            min_certainty(
                ['likely', upstream_metadata['Repository'].certainty]))


def _extrapolate_bug_submit_from_bug_db(
        upstream_metadata, net_access):
    bug_submit_url = bug_submit_url_from_bug_database_url(
        upstream_metadata['Bug-Database'].value)
    if bug_submit_url:
        return UpstreamDatum(
            'Bug-Submit', bug_submit_url,
            upstream_metadata['Bug-Database'].certainty)


def _extrapolate_bug_db_from_bug_submit(
        upstream_metadata, net_access):
    bug_db_url = bug_database_url_from_bug_submit_url(
        upstream_metadata['Bug-Submit'].value)
    if bug_db_url:
        return UpstreamDatum(
            'Bug-Database', bug_db_url,
            upstream_metadata['Bug-Submit'].certainty)


def _copy_bug_db_field(upstream_metadata, net_access):
    ret = UpstreamDatum(
        'Bug-Database',
        upstream_metadata['Bugs-Database'].value,
        upstream_metadata['Bugs-Database'].certainty,
        upstream_metadata['Bugs-Database'].origin)
    del upstream_metadata['Bugs-Database']
    return ret


def _extrapolate_security_contact_from_security_md(
        upstream_metadata, net_access):
    repository_url = upstream_metadata['Repository']
    security_md_path = upstream_metadata['X-Security-MD']
    security_url = browse_url_from_repo_url(
        repository_url.value, security_md_path.value)
    if security_url is None:
        return None
    return UpstreamDatum(
        'Security-Contact', security_url,
        certainty=min_certainty(
            [repository_url.certainty, security_md_path.certainty]),
        origin=security_md_path.origin)


def _extrapolate_contact_from_maintainer(upstream_metadata, net_access):
    maintainer = upstream_metadata['X-Maintainer']
    return UpstreamDatum(
        'Contact', str(maintainer.value),
        certainty=min_certainty([maintainer.certainty]),
        origin=maintainer.origin)


def _extrapolate_homepage_from_repository_browse(
        upstream_metadata, net_access):
    browse_url = upstream_metadata['Repository-Browse'].value
    parsed = urlparse(browse_url)
    # Some hosting sites are commonly used as Homepage
    # TODO(jelmer): Maybe check that there is a README file that
    # can serve as index?
    if parsed.netloc in ('github.com', ) or is_gitlab_site(parsed.netloc):
        return UpstreamDatum('Homepage', browse_url, 'possible')


EXTRAPOLATE_FNS = [
    (['Homepage'], 'Repository', _extrapolate_repository_from_homepage),
    (['Repository-Browse'], 'Homepage',
     _extrapolate_homepage_from_repository_browse),
    (['Bugs-Database'], 'Bug-Database', _copy_bug_db_field),
    (['Bug-Database'], 'Repository', _extrapolate_repository_from_bug_db),
    (['Repository'], 'Repository-Browse',
     _extrapolate_repository_browse_from_repository),
    (['Repository-Browse'], 'Repository',
     _extrapolate_repository_from_repository_browse),
    (['Repository'], 'Bug-Database',
     _extrapolate_bug_database_from_repository),
    (['Bug-Database'], 'Bug-Submit', _extrapolate_bug_submit_from_bug_db),
    (['Bug-Submit'], 'Bug-Database', _extrapolate_bug_db_from_bug_submit),
    (['X-Download'], 'Repository', _extrapolate_repository_from_download),
    (['Repository'], 'Name', _extrapolate_name_from_repository),
    (['Repository', 'X-Security-MD'],
     'Security-Contact', _extrapolate_security_contact_from_security_md),
    (['X-Maintainer'], 'Contact',
     _extrapolate_contact_from_maintainer),
]


def extend_upstream_metadata(upstream_metadata,  # noqa: C901
                             path, minimum_certainty=None,
                             net_access=False,
                             consult_external_directory=False):
    """Extend a set of upstream metadata.
    """
    # TODO(jelmer): Use EXTRAPOLATE_FNS mechanism for this?
    for field in ['Homepage', 'Bug-Database', 'Bug-Submit', 'Repository',
                  'Repository-Browse', 'X-Download']:
        if field not in upstream_metadata:
            continue
        project = extract_sf_project_name(upstream_metadata[field].value)
        if project:
            certainty = min_certainty(
                ['likely', upstream_metadata[field].certainty])
            upstream_metadata['Archive'] = UpstreamDatum(
                'Archive', 'SourceForge', certainty)
            upstream_metadata['X-SourceForge-Project'] = UpstreamDatum(
                'X-SourceForge-Project', project, certainty)
            break

    archive = upstream_metadata.get('Archive')
    if (archive and archive.value == 'SourceForge' and
            'X-SourceForge-Project' in upstream_metadata and
            net_access):
        sf_project = upstream_metadata['X-SourceForge-Project'].value
        try:
            extend_from_sf(upstream_metadata, sf_project)
        except NoSuchSourceForgeProject:
            del upstream_metadata['X-SourceForge-Project']

    if (archive and archive.value == 'Hackage' and
            'X-Hackage-Package' in upstream_metadata and
            net_access):
        hackage_package = upstream_metadata['X-Hackage-Package'].value
        try:
            extend_from_hackage(upstream_metadata, hackage_package)
        except NoSuchHackagePackage:
            del upstream_metadata['X-Hackage-Package']

    if (archive and archive.value == 'crates.io' and
            'X-Cargo-Crate' in upstream_metadata and
            net_access):
        crate = upstream_metadata['X-Cargo-Crate'].value
        try:
            extend_from_crates_io(upstream_metadata, crate)
        except NoSuchCrate:
            del upstream_metadata['X-Cargo-Crate']

    if net_access and consult_external_directory:
        # TODO(jelmer): Don't assume debian/control exists
        from debian.deb822 import Deb822

        try:
            with open(os.path.join(path, 'debian/control'), 'r') as f:
                package = Deb822(f)['Source']
        except FileNotFoundError:
            # Huh, okay.
            pass
        else:
            extend_from_lp(upstream_metadata, minimum_certainty, package)
            extend_from_aur(upstream_metadata, minimum_certainty, package)
            extend_from_repology(upstream_metadata, minimum_certainty, package)
    pecl_url = upstream_metadata.get('X-Pecl-URL')
    if net_access and pecl_url:
        extend_from_pecl(upstream_metadata, pecl_url.value, pecl_url.certainty)
    changed = True
    while changed:
        changed = False
        for from_fields, to_field, fn in EXTRAPOLATE_FNS:
            from_certainties = []
            for from_field in from_fields:
                try:
                    from_value = upstream_metadata[from_field]
                except KeyError:
                    from_certainties = None
                    break
                from_certainties.append(from_value.certainty)
            if not from_certainties:
                # Nope
                continue
            from_certainty = min_certainty(from_certainties)
            old_to_value = upstream_metadata.get(to_field)
            if old_to_value is not None and (
                    certainty_to_confidence(from_certainty) >=
                    certainty_to_confidence(old_to_value.certainty)):
                continue
            result = fn(upstream_metadata, net_access)
            if not result:
                continue
            if not certainty_sufficient(result.certainty, minimum_certainty):
                continue
            if old_to_value is None or (
                    certainty_to_confidence(result.certainty) <
                    certainty_to_confidence(old_to_value.certainty)):
                upstream_metadata[to_field] = result
                changed = True


def verify_screenshots(urls):
    headers = {'User-Agent': USER_AGENT}
    for url in urls:
        try:
            response = urlopen(
                Request(url, headers=headers, method='HEAD'),
                timeout=DEFAULT_URLLIB_TIMEOUT)
        except urllib.error.HTTPError as e:
            if e.code == 404:
                yield url, False
            else:
                yield url, None
        else:
            assert response is not None
            # TODO(jelmer): Check content-type?
            yield url, True


def check_upstream_metadata(upstream_metadata, version=None):
    """Check upstream metadata.

    This will make network connections, etc.
    """
    repository = upstream_metadata.get('Repository')
    if repository and repository.certainty == 'likely':
        if verify_repository_url(repository.value, version=version):
            repository.certainty = 'certain'
            derived_browse_url = browse_url_from_repo_url(repository.value)
            browse_repo = upstream_metadata.get('Repository-Browse')
            if browse_repo and derived_browse_url == browse_repo.value:
                browse_repo.certainty = repository.certainty
        else:
            # TODO(jelmer): Remove altogether, or downgrade to a lesser
            # certainty?
            pass
    bug_database = upstream_metadata.get('Bug-Database')
    if bug_database and bug_database.certainty == 'likely':
        if verify_bug_database_url(bug_database.value):
            bug_database.certainty = 'certain'
    bug_submit = upstream_metadata.get('Bug-Submit')
    if bug_submit and bug_submit.certainty == 'likely':
        if verify_bug_submit_url(bug_submit.value):
            bug_submit.certainty = 'certain'
    screenshots = upstream_metadata.get('Screenshots')
    if screenshots and screenshots.certainty == 'likely':
        newvalue = []
        screenshots.certainty = 'certain'
        for i, (url, status) in enumerate(verify_screenshots(
                screenshots.value)):
            if status is True:
                newvalue.append(url)
            elif status is False:
                pass
            else:
                screenshots.certainty = 'likely'
        screenshots.value = newvalue


def parse_pkgbuild_variables(f):
    import shlex
    variables = {}
    keep = None
    existing = None
    for line in f:
        if existing:
            line = existing + line
        if line.endswith(b'\\\n'):
            existing = line[:-2]
            continue
        existing = None
        if (line.startswith(b'\t') or line.startswith(b' ') or
                line.startswith(b'#')):
            continue
        if keep:
            keep = (keep[0], keep[1] + line)
            if line.rstrip().endswith(b')'):
                variables[keep[0].decode()] = shlex.split(
                    keep[1].rstrip(b'\n').decode())
                keep = None
            continue
        try:
            (key, value) = line.split(b'=', 1)
        except ValueError:
            continue
        if value.startswith(b'('):
            if value.rstrip().endswith(b')'):
                value = value.rstrip()[1:-1]
            else:
                keep = (key, value[1:])
                continue
        variables[key.decode()] = shlex.split(value.rstrip(b'\n').decode())
    return variables


def guess_from_pecl(package):
    if not package.startswith('php-'):
        return iter([])
    php_package = package[4:]
    url = 'https://pecl.php.net/packages/%s' % php_package.replace('-', '_')
    data = dict(guess_from_pecl_url(url))
    try:
        data['Repository'] = guess_repo_from_url(
                data['Repository-Browse'], net_access=True)
    except KeyError:
        pass
    return data.items()


def guess_from_pecl_url(url):
    headers = {'User-Agent': USER_AGENT}
    try:
        f = urlopen(
            Request(url, headers=headers),
            timeout=PECL_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        return
    except socket.timeout:
        logging.warning('timeout contacting pecl, ignoring: %s', url)
        return
    try:
        from bs4 import BeautifulSoup
    except ModuleNotFoundError:
        logging.warning(
            'bs4 missing so unable to scan pecl page, ignoring %s', url)
        return
    bs = BeautifulSoup(f.read(), features='lxml')
    tag = bs.find('a', text='Browse Source')
    if tag is not None:
        yield 'Repository-Browse', tag.attrs['href']
    tag = bs.find('a', text='Package Bugs')
    if tag is not None:
        yield 'Bug-Database', tag.attrs['href']
    label_tag = bs.find('th', text='Homepage')
    if label_tag is not None:
        tag = label_tag.parent.find('a')
        if tag is not None:
            yield 'Homepage', tag.attrs['href']


def strip_vcs_prefixes(url):
    for prefix in ['git', 'hg']:
        if url.startswith(prefix+'+'):
            return url[len(prefix)+1:]
    return url


def guess_from_aur(package: str):
    vcses = ['git', 'bzr', 'hg']
    for vcs in vcses:
        url = (
            'https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=%s-%s' %
            (package, vcs))
        headers = {'User-Agent': USER_AGENT}
        try:
            f = urlopen(
                Request(url, headers=headers),
                timeout=DEFAULT_URLLIB_TIMEOUT)
        except urllib.error.HTTPError as e:
            if e.code != 404:
                raise
            continue
        else:
            break
    else:
        return

    variables = parse_pkgbuild_variables(f)
    for key, value in variables.items():
        if key == 'url':
            yield 'Homepage', value[0]
        if key == 'source':
            if not value:
                continue
            value = value[0]
            if "${" in value:
                for k, v in variables.items():
                    value = value.replace('${%s}' % k, ' '.join(v))
                    value = value.replace('$%s' % k, ' '.join(v))
            try:
                unique_name, url = value.split('::', 1)
            except ValueError:
                url = value
            url = url.replace('#branch=', ',branch=')
            if any([url.startswith(vcs+'+') for vcs in vcses]):
                yield 'Repository', strip_vcs_prefixes(url)
        if key == '_gitroot':
            repo_url = value[0]
            yield 'Repository', strip_vcs_prefixes(repo_url)


def guess_from_launchpad(package, distribution=None, suite=None):  # noqa: C901
    if distribution is None:
        # Default to Ubuntu; it's got more fields populated.
        distribution = 'ubuntu'
    if suite is None:
        if distribution == 'ubuntu':
            from distro_info import UbuntuDistroInfo, DistroDataOutdated
            ubuntu = UbuntuDistroInfo()
            try:
                suite = ubuntu.devel()
            except DistroDataOutdated as e:
                logging.warning('%s', str(e))
                suite = ubuntu.all[-1]
        elif distribution == 'debian':
            suite = 'sid'
    sourcepackage_url = (
        'https://api.launchpad.net/devel/%(distribution)s/'
        '%(suite)s/+source/%(package)s' % {
            'package': package,
            'suite': suite,
            'distribution': distribution})
    try:
        sourcepackage_data = _load_json_url(sourcepackage_url)
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        return
    except socket.timeout:
        logging.warning('timeout contacting launchpad, ignoring')
        return

    productseries_url = sourcepackage_data.get('productseries_link')
    if not productseries_url:
        return
    productseries_data = _load_json_url(productseries_url)
    project_link = productseries_data['project_link']
    project_data = _load_json_url(project_link)
    if project_data.get('homepage_url'):
        yield 'Homepage', project_data['homepage_url']
    yield 'Name', project_data['display_name']
    if project_data.get('sourceforge_project'):
        yield ('X-SourceForge-Project', project_data['sourceforge_project'])
    if project_data.get('wiki_url'):
        yield ('X-Wiki', project_data['wiki_url'])
    if project_data.get('summary'):
        yield ('X-Summary', project_data['summary'])
    if project_data['vcs'] == 'Bazaar':
        branch_link = productseries_data.get('branch_link')
        if branch_link:
            try:
                code_import_data = _load_json_url(
                    branch_link + '/+code-import')
                if code_import_data['url']:
                    # Sometimes this URL is not set, e.g. for CVS repositories.
                    yield 'Repository', code_import_data['url']
            except urllib.error.HTTPError as e:
                if e.code != 404:
                    raise
                if project_data['official_codehosting']:
                    try:
                        branch_data = _load_json_url(branch_link)
                    except urllib.error.HTTPError as e:
                        if e.code != 404:
                            raise
                        branch_data = None
                    if branch_data:
                        yield 'Archive', 'launchpad'
                        yield 'Repository', branch_data['bzr_identity']
                        yield 'Repository-Browse', branch_data['web_link']
    elif project_data['vcs'] == 'Git':
        repo_link = (
            'https://api.launchpad.net/devel/+git?ws.op=getByPath&path=%s' %
            project_data['name'])
        repo_data = _load_json_url(repo_link)
        if not repo_data:
            return
        code_import_link = repo_data.get('code_import_link')
        if code_import_link:
            code_import_data = _load_json_url(repo_data['code_import_link'])
            if code_import_data['url']:
                # Sometimes this URL is not set, e.g. for CVS repositories.
                yield 'Repository', code_import_data['url']
        else:
            if project_data['official_codehosting']:
                yield 'Archive', 'launchpad'
                yield 'Repository', repo_data['git_https_url']
                yield 'Repository-Browse', repo_data['web_link']
    elif project_data.get('vcs') is not None:
        raise AssertionError('unknown vcs: %r' % project_data['vcs'])


def fix_upstream_metadata(upstream_metadata):
    """Fix existing upstream metadata."""
    if 'Repository' in upstream_metadata:
        repo = upstream_metadata['Repository']
        url = repo.value
        url = sanitize_vcs_url(url)
        repo.value = url
    if 'X-Summary' in upstream_metadata:
        summary = upstream_metadata['X-Summary']
        summary.value = summary.value.split('. ')[0]
        summary.value = summary.value.rstrip().rstrip('.')
