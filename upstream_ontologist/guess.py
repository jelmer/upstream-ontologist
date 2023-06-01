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

from functools import partial
import logging
import operator
import os
import re
import socket
import urllib.error
from typing import Optional, Iterable, List, Iterator, Any, Dict, Tuple, cast, Callable, Type
from urllib.parse import urlparse, urlunparse, urljoin
from urllib.request import urlopen, Request

from . import _upstream_ontologist

from .vcs import (
    unsplit_vcs_url,
    browse_url_from_repo_url,
    plausible_browse_url as plausible_vcs_browse_url,
    sanitize_url as sanitize_vcs_url,
    is_gitlab_site,
    guess_repo_from_url,
    check_repository_url_canonical,
)

from . import (
    DEFAULT_URLLIB_TIMEOUT,
    USER_AGENT,
    UpstreamDatum,
    UpstreamMetadata,
    min_certainty,
    certainty_to_confidence,
    certainty_sufficient,
    _load_json_url,
    Person,
    InvalidUrl,
    UrlUnverifiable,
)


# Pecl is quite slow, so up the timeout a bit.
PECL_URLLIB_TIMEOUT = 15


logger = logging.getLogger(__name__)


def warn_missing_dependency(path, module_name):
    logger.warning(
        'Not scanning %s, because the python module %s is not available',
        path, module_name)


class NoSuchForgeProject(Exception):

    def __init__(self, project):
        self.project = project


def get_sf_metadata(project):
    url = 'https://sourceforge.net/rest/p/%s' % project
    try:
        return _load_json_url(url)
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        raise NoSuchForgeProject(project) from e


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
        raise NoSuchRepologyProject(srcname) from e


DATUM_TYPES = {
    'Bug-Submit': str,
    'Bug-Database': str,
    'Repository': str,
    'Repository-Browse': str,
    'Documentation': str,
    'Keywords': list,
    'License': str,
    'Go-Import-Path': str,
    'Summary': str,
    'Description': str,
    'Wiki': str,
    'SourceForge-Project': str,
    'Archive': str,
    'Homepage': str,
    'Name': str,
    'Version': str,
    'Download': str,
    'Pecl-Package': str,
    'Screenshots': list,
    'Contact': str,
    'Author': list,
    'Security-MD': str,
    # TODO(jelmer): Allow multiple maintainers?
    'Maintainer': Person,
    'Cargo-Crate': str,
    'API-Documentation': str,
    'Funding': str,
    'GitHub-Project': str,

    # We should possibly hide these:
    'Debian-ITP': int,
}


def known_bad_url(value):
    if '${' in value:
        return True
    return False


def known_bad_guess(datum: UpstreamDatum) -> bool:  # noqa: C901
    try:
        expected_type = DATUM_TYPES[datum.field]
    except KeyError:
        logger.warning('Unknown field %s', datum.field)
        return False
    if not isinstance(datum.value, expected_type):
        logger.warning(
            'filtering out bad value %r for %s',
            datum.value, datum.field)
        return True
    if datum.field in ('Bug-Submit', 'Bug-Database'):
        assert isinstance(datum.value, str)
        if known_bad_url(datum.value):
            return True
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'bugzilla.gnome.org':
            return True
        if parsed_url.hostname == 'bugs.freedesktop.org':
            return True
        if parsed_url.path.endswith('/sign_in'):
            return True
    if datum.field == 'Repository':
        assert isinstance(datum.value, str)
        if known_bad_url(datum.value):
            return True
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'anongit.kde.org':
            return True
        if parsed_url.hostname == 'git.gitorious.org':
            return True
        if parsed_url.path.endswith('/sign_in'):
            return True
    if datum.field == 'Homepage':
        assert isinstance(datum.value, str)
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname in ('pypi.org', 'rubygems.org'):
            return True
    if datum.field == 'Repository-Browse':
        assert isinstance(datum.value, str)
        if known_bad_url(datum.value):
            return True
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'cgit.kde.org':
            return True
        if parsed_url.path.endswith('/sign_in'):
            return True
    if datum.field == 'Author':
        assert isinstance(datum.value, list)
        for value in datum.value:
            if value.name is not None:
                if 'Maintainer' in value.name:
                    return True
                if 'Contributor' in value.name:
                    return True
    if datum.field == 'Name':
        assert isinstance(datum.value, str)
        if datum.value.lower() == 'package':
            return True
    if datum.field == 'Version':
        assert isinstance(datum.value, str)
        if datum.value.lower() in ('devel', ):
            return True
    if isinstance(datum.value, str) and datum.value.strip().lower() == 'unknown':
        return True
    return False


def filter_bad_guesses(
        guessed_items: Iterable[UpstreamDatum]) -> Iterator[UpstreamDatum]:
    for item in guessed_items:
        if known_bad_guess(item):
            logger.debug('Excluding known bad item %r', item)
        else:
            yield item


def update_from_guesses(upstream_metadata: UpstreamMetadata,
                        guessed_items: Iterable[UpstreamDatum]):
    changed = []
    for datum in guessed_items:
        current_datum: Optional[UpstreamDatum] = cast(
            Optional[UpstreamDatum], upstream_metadata.get(datum.field))
        if not current_datum or (
                certainty_to_confidence(datum.certainty)  # type: ignore
                < certainty_to_confidence(current_datum.certainty)):
            upstream_metadata[datum.field] = datum  # type: ignore
            changed.append(datum)
    return changed


def guess_from_debian_rules(path, trust_package):
    try:
        from debmutate._rules import Makefile
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return
    mf = Makefile.from_path(path)
    try:
        upstream_git = mf.get_variable(b'UPSTREAM_GIT')
    except KeyError:
        pass
    else:
        yield UpstreamDatum[str](
            "Repository", upstream_git.decode(), "likely")
    try:
        upstream_url = mf.get_variable(b'DEB_UPSTREAM_URL')
    except KeyError:
        pass
    else:
        yield UpstreamDatum("Download", upstream_url.decode(), "likely")


def extract_pecl_package_name(url):
    m = re.match('https?://pecl.php.net/package/(.*)', url)
    if m:
        return m.group(1)
    return None


def _metadata_from_url(url: str, origin=None):
    """Obtain metadata from a URL related to the project.

    Args:
      url: The URL to inspect
      origin: Origin to report for metadata
    """
    sf_project = extract_sf_project_name(url)
    if sf_project:
        yield UpstreamDatum(
            "Archive", "SourceForge", "certain",
            origin=origin)
        yield UpstreamDatum(
            "SourceForge-Project", sf_project, "certain",
            origin=origin)
    pecl_package = extract_pecl_package_name(url)
    if pecl_package:
        yield UpstreamDatum(
            "Archive", "Pecl", "certain",
            origin=origin)
        yield UpstreamDatum(
            'Pecl-Package', pecl_package, 'certain', origin=origin)


def guess_from_debian_watch(path, trust_package):
    try:
        from debmutate.watch import (
            parse_watch_file,
            MissingVersion,
        )
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return

    try:
        from debian.deb822 import Deb822
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return

    def get_package_name():
        with open(os.path.join(os.path.dirname(path), 'control')) as f:
            return Deb822(f)['Source']
    with open(path) as f:
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
            yield from _metadata_from_url(url, origin=path)
            m = re.match(
                'https?://hackage.haskell.org/package/(.*)/distro-monitor',
                url)
            if m:
                yield UpstreamDatum(
                    "Archive", "Hackage", "certain", origin=path)
                yield UpstreamDatum(
                    "Hackage-Package", m.group(1), "certain", origin=path)


debian_is_native = _upstream_ontologist.debian_is_native


def guess_from_debian_control(path, trust_package):
    try:
        from debian.deb822 import Deb822
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return
    with open(path) as f:
        source = Deb822(f)
        is_native = debian_is_native(os.path.dirname(path))
        if 'Homepage' in source:
            yield UpstreamDatum('Homepage', source['Homepage'], 'certain')
        if 'XS-Go-Import-Path' in source:
            yield UpstreamDatum(
                'Go-Import-Path', source['XS-Go-Import-Path'], 'certain')
            yield (
                UpstreamDatum(
                    'Repository',
                    'https://' + source['XS-Go-Import-Path'],
                    'likely'))
        if is_native:
            if 'Vcs-Git' in source:
                yield UpstreamDatum('Repository', source['Vcs-Git'], 'certain')
            if 'Vcs-Browse' in source:
                yield UpstreamDatum('Repository-Browse', source['Vcs-Browse'], 'certain')
        other_paras = list(Deb822.iter_paragraphs(f))
        if len(other_paras) == 1 and is_native:
            certainty = "certain"
        elif len(other_paras) > 1 and is_native:
            certainty = "possible"
        elif len(other_paras) == 1 and not is_native:
            certainty = "confident"
        else:
            certainty = "likely"
        for para in other_paras:
            if 'Description' in para:
                lines = para['Description'].splitlines(True)
                summary = lines[0].rstrip('\n')
                description_lines = [
                    ('\n' if line == ' .\n' else line[1:]) for line in lines[1:]]
                if (description_lines
                        and description_lines[-1].startswith('This package contains ')):
                    if ' - ' in summary:
                        summary = summary.rsplit(' - ', 1)[0]
                    del description_lines[-1]
                if description_lines and not description_lines[-1].strip():
                    del description_lines[-1]
                if summary:
                    yield UpstreamDatum(
                        'Summary', summary, certainty)
                if description_lines:
                    yield UpstreamDatum(
                        'Description',
                        ''.join(description_lines), certainty)


def guess_from_debian_changelog(path, trust_package):
    try:
        from debian.changelog import Changelog
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return
    with open(path, 'rb') as f:
        cl = Changelog(f)
    source = cl.package
    yield UpstreamDatum('Name', cl.package, 'confident')
    yield UpstreamDatum('Version', cl.version.upstream_version, 'confident')
    if source.startswith('rust-'):
        semver_suffix: Optional[bool]
        try:
            from toml.decoder import load as load_toml
            with open('debian/debcargo.toml') as f:
                debcargo = load_toml(f)
        except FileNotFoundError:
            semver_suffix = False
        else:
            semver_suffix = debcargo.get('semver_suffix', False)
            if not isinstance(semver_suffix, bool):
                logging.warning(
                    'Unexpected setting for semver_suffix: %r, resetting to False',
                    semver_suffix)
                semver_suffix = False
        from debmutate.debcargo import parse_debcargo_source_name, cargo_translate_dashes
        crate, crate_semver_version = parse_debcargo_source_name(
            source, semver_suffix)
        if '-' in crate:
            crate = cargo_translate_dashes(crate)
        yield UpstreamDatum('Archive', 'crates.io', 'certain')
        yield UpstreamDatum('Cargo-Crate', crate, 'certain')

    # Find the ITP
    itp = None
    for change in cl[-1].changes():
        m = re.match(r'[\s\*]*Initial Release.*Closes: \#([0-9]+).*', change, re.I)
        if m:
            itp = int(m.group(1))
    if itp:
        yield UpstreamDatum('Debian-ITP', itp, 'certain')
        try:
            import debianbts
        except ModuleNotFoundError as e:
            warn_missing_dependency(path, e.name)
            return
        else:
            import pysimplesoap

            logger.debug('Retrieving Debian bug %d', itp)
            try:
                orig = debianbts.get_bug_log(itp)[0]
            except pysimplesoap.client.SoapFault as e:
                logger.warning('Unable to get info about %d: %s' % (itp, e))
            except (TypeError, ValueError):
                # Almost certainly a broken pysimplesoap bug :(
                logger.exception('Error getting bug log')
            else:
                yield from metadata_from_itp_bug_body(orig['body'])


metadata_from_itp_bug_body = _upstream_ontologist.metadata_from_itp_bug_body


def guess_from_python_metadata(pkg_info):
    for field, value in pkg_info.items():
        if field == 'Name':
            yield UpstreamDatum('Name', value, 'certain')
        elif field == 'Version':
            yield UpstreamDatum('Version', value, 'certain')
        elif field == 'Home-page':
            yield UpstreamDatum('Homepage', value, 'certain')
        elif field == 'Project-URL':
            url_type, url = value.split(', ')
            yield from parse_python_project_urls({url_type: url})
        elif field == 'Summary':
            yield UpstreamDatum('Summary', value, 'certain')
        elif field == 'Author':
            author_email = pkg_info.get('Author-email')
            author = Person(value, author_email)
            yield UpstreamDatum('Author', [author], 'certain')
        elif field == 'License':
            yield UpstreamDatum('License', value, 'certain')
        elif field == 'Download-URL':
            yield UpstreamDatum('Download', value, 'certain')
        elif field in ('Author-email', 'Classifier', 'Requires-Python',
                       'License-File', 'Metadata-Version',
                       'Provides-Extra', 'Description-Content-Type'):
            pass
        else:
            logger.debug('Unknown PKG-INFO field %s (%r)', field, value)
    yield from parse_python_long_description(
        pkg_info.get_payload(), pkg_info.get_content_type())


def guess_from_pkg_info(path, trust_package):
    """Get the metadata from a PKG-INFO file."""
    from email.parser import Parser
    try:
        with open(path) as f:
            pkg_info = Parser().parse(f)
    except FileNotFoundError:
        return
    yield from guess_from_python_metadata(pkg_info)


def parse_python_long_description(long_description, content_type) -> Iterator[UpstreamDatum]:
    description: Optional[str]
    if long_description in (None, ''):
        return
    # Discard encoding, etc.
    if content_type:
        content_type = content_type.split(';')[0]
    if '-*-restructuredtext-*-' in long_description.splitlines()[0]:
        content_type = 'text/restructured-text'
    extra_md: Iterable[UpstreamDatum]
    if content_type in (None, 'text/plain'):
        if len(long_description.splitlines()) > 30:
            return
        yield UpstreamDatum(
            'Description', long_description, 'possible')
        extra_md = []
    elif content_type in ('text/restructured-text', 'text/x-rst'):
        from .readme import description_from_readme_rst
        description, extra_md = description_from_readme_rst(long_description)
        if description:
            yield UpstreamDatum('Description', description, 'possible')
    elif content_type == 'text/markdown':
        from .readme import description_from_readme_md
        description, extra_md = description_from_readme_md(long_description)
        if description:
            yield UpstreamDatum('Description', description, 'possible')
    else:
        extra_md = []
    yield from extra_md


def guess_from_setup_cfg(path, trust_package):
    try:
        from setuptools.config.setupcfg import read_configuration
    except ImportError:  # older setuptools
        from setuptools.config import read_configuration  # type: ignore
    # read_configuration needs a function cwd
    try:
        os.getcwd()
    except FileNotFoundError:
        os.chdir(os.path.dirname(path))
    config = read_configuration(path)
    metadata = config.get('metadata')
    if metadata:
        for field, value in metadata.items():
            if field == 'name':
                yield UpstreamDatum('Name', value, 'certain')
            elif field == 'version':
                yield UpstreamDatum('Name', value, 'certain')
            elif field == 'url':
                yield from parse_python_url(value)
            elif field == 'description':
                yield UpstreamDatum('Summary', value, 'certain')
            elif field == 'long_description':
                yield from parse_python_long_description(
                    value,
                    metadata.get('long_description_content_type'))
            elif field == 'maintainer':
                yield UpstreamDatum(
                    'Maintainer',
                    Person(name=value, email=metadata.get('maintainer_email')),
                    'certain')
            elif field == 'author':
                yield UpstreamDatum(
                    'Author',
                    [Person(name=value, email=metadata.get('author_email'))],
                    'certain')
            elif field == 'project_urls':
                yield from parse_python_project_urls(value)
            elif field in ('long_description_content_type', 'maintainer_email',
                           'author_email'):
                pass
            else:
                logger.debug('Unknown setup.cfg field %s (%r)', field, value)


def parse_python_url(url):
    repo = guess_repo_from_url(url)
    if repo:
        yield UpstreamDatum('Repository', repo, 'likely')
    yield UpstreamDatum('Homepage', url, 'likely')


def guess_from_setup_py_executed(path):
    # Import setuptools, just in case it replaces distutils
    try:
        import setuptools  # noqa: F401
    except ModuleNotFoundError:
        pass
    from distutils.core import run_setup
    orig = os.getcwd()
    result: Any
    try:
        os.chdir(os.path.dirname(path))
        result = run_setup(os.path.abspath(path), stop_after="config")
    finally:
        os.chdir(orig)
    if result.get_name() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum('Name', result.get_name(), 'certain')
    if result.get_version() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum('Version', result.get_version(), 'certain')
    if result.get_url() not in (None, '', 'UNKNOWN'):
        yield from parse_python_url(result.get_url())
    if result.get_download_url() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum(
            'Download', result.get_download_url(), 'likely')
    if result.get_license() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum(
            'License', result.get_license(), 'likely')
    if result.get_contact() not in (None, '', 'UNKNOWN'):
        contact = result.get_contact()
        if result.get_contact_email() not in (None, '', 'UNKNOWN'):
            contact += " <%s>" % result.get_contact_email()
        yield UpstreamDatum('Contact', contact, 'likely')
    if result.get_description() not in (None, '', 'UNKNOWN'):
        yield UpstreamDatum('Summary', result.get_description(), 'certain')
    if result.metadata.long_description not in (None, '', 'UNKNOWN'):
        yield from parse_python_long_description(
            result.metadata.long_description,
            getattr(result.metadata, 'long_description_content_type', None))
    yield from parse_python_project_urls(getattr(result.metadata, 'project_urls', {}))


def parse_python_project_urls(urls):
    for url_type, url in urls.items():
        if url_type in ('GitHub', 'Repository', 'Source Code', 'Source'):
            yield UpstreamDatum(
                'Repository', str(url), 'certain')
        elif url_type in ('Bug Tracker', 'Bug Reports'):
            yield UpstreamDatum(
                'Bug-Database', str(url), 'certain')
        elif url_type in ('Documentation', ):
            yield UpstreamDatum(
                'Documentation', str(url), 'certain')
        elif url_type in ('Funding', ):
            yield UpstreamDatum(
                'Funding', str(url), 'certain')
        else:
            logger.debug(
                'Unknown Python project URL type: %s', url_type)


def guess_from_setup_py(path, trust_package):  # noqa: C901
    if trust_package:
        try:
            yield from guess_from_setup_py_executed(path)
        except Exception as e:
            logger.warning('Failed to run setup.py: %r', e)
        else:
            return
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
        logger.warning('Syntax error while parsing setup.py: %s', e)
        return
    setup_args = {}

    for statement in tree.body:
        # We only care about function calls or assignments to functions named
        # `setup` or `main`
        if (isinstance(statement, (ast.Expr, ast.Call, ast.Assign))
            and isinstance(statement.value, ast.Call)
            and isinstance(statement.value.func, ast.Name)
            # we also look for main as sometimes this is used instead of
            # setup()
                and statement.value.func.id in ('setup', 'main')):

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
                    for (key, value) in zip(kw.value.keys, kw.value.values):  # type: ignore
                        if isinstance(key, ast.Str) and isinstance(value, (ast.Str, ast.Constant)):
                            setup_args[key.s] = value.s

                # TODO: what if kw.value is an expression like a call to
                # version=get_version or version__version__

    # End code from https://github.com/nexB/scancode-toolkit/blob/develop/src/packagedcode/pypi.py

    if 'name' in setup_args:
        yield UpstreamDatum('Name', setup_args['name'], 'certain')
    if 'version' in setup_args:
        yield UpstreamDatum('Version', setup_args['version'], 'certain')
    if 'description' in setup_args:
        yield UpstreamDatum('Summary', setup_args['description'], 'certain')
    if 'long_description' in setup_args:
        yield from parse_python_long_description(
            setup_args['long_description'], setup_args.get('long_description_content_type'))
    if 'license' in setup_args:
        yield UpstreamDatum('License', setup_args['license'], 'certain')
    if 'download_url' in setup_args and setup_args.get('download_url'):
        yield UpstreamDatum('Download', setup_args['download_url'], 'certain')
    if 'url' in setup_args:
        yield from parse_python_url(setup_args['url'])
    if 'project_urls' in setup_args:
        yield from parse_python_project_urls(setup_args['project_urls'])
    if 'maintainer' in setup_args:
        maintainer_email = setup_args.get('maintainer_email')
        maintainer = setup_args['maintainer']
        if isinstance(maintainer, list) and len(maintainer) == 1:
            maintainer = maintainer[0]
        if isinstance(maintainer, str):
            maintainer = Person(maintainer, maintainer_email)
            yield UpstreamDatum('Maintainer', maintainer, 'certain')
    if 'author' in setup_args:
        author_email = setup_args.get('author_email')
        author = setup_args['author']
        if isinstance(author, str):
            authors = [author]
            author_emails = [author_email]
        elif isinstance(author, list):
            authors = author
            author_emails = author_email  # type: ignore
        yield UpstreamDatum(
            'Author',
            [Person(author, email)
             for (author, email) in zip(authors, author_emails)],
            'certain')


guess_from_composer_json = _upstream_ontologist.guess_from_composer_json
guess_from_package_json = _upstream_ontologist.guess_from_package_json


def xmlparse_simplify_namespaces(path, namespaces):
    import xml.etree.ElementTree as ET
    namespaces = ['{%s}' % ns for ns in namespaces]
    tree = ET.iterparse(path)
    for _, el in tree:
        for namespace in namespaces:
            el.tag = el.tag.replace(namespace, '')
    return tree.root  # type: ignore


guess_from_package_xml = _upstream_ontologist.guess_from_package_xml


guess_from_pod = _upstream_ontologist.guess_from_pod
guess_from_perl_module = _upstream_ontologist.guess_from_perl_module
guess_from_perl_dist_name = _upstream_ontologist.guess_from_perl_dist_name
guess_from_dist_ini = _upstream_ontologist.guess_from_dist_ini


def guess_from_debian_copyright(path, trust_package):  # noqa: C901
    try:
        from debian.copyright import (
            Copyright,
            NotMachineReadableError,
            MachineReadableFormatError,
        )
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return
    from_urls = []
    with open(path) as f:
        try:
            copyright = Copyright(f, strict=False)
        except NotMachineReadableError:
            header = None
        except MachineReadableFormatError as e:
            logger.warning('Error parsing copyright file: %s', e)
            header = None
        except ValueError as e:
            # This can happen with an error message of
            # ValueError: value must not have blank lines
            logger.warning('Error parsing copyright file: %s', e)
            header = None
        else:
            header = copyright.header
    if header:
        if header.upstream_name:
            certainty = 'certain'
            if ' ' in header.upstream_name:
                certainty = 'confident'
            yield UpstreamDatum("Name", header.upstream_name, certainty)
        if header.upstream_contact:
            yield UpstreamDatum(
                "Contact", ','.join(header.upstream_contact), 'certain')
        if header.source:
            if ' ' in header.source:
                from_urls.extend([u for u in re.split('[ ,\n]', header.source) if u])  # type: ignore
            else:
                from_urls.append(header.source)
        if "X-Upstream-Bugs" in header:  # type: ignore
            yield UpstreamDatum(
                "Bug-Database", header["X-Upstream-Bugs"], 'certain')
        if "X-Source-Downloaded-From" in header:  # type: ignore
            url = guess_repo_from_url(header["X-Source-Downloaded-From"])
            if url is not None:
                yield UpstreamDatum("Repository", url, 'certain')
        if header.source:
            from_urls.extend(
                [m.group(0)
                 for m in
                 re.finditer(r'((http|https):\/\/([^ ]+))', header.source)])  # type: ignore
        referenced_licenses = set()
        for para in copyright.all_paragraphs():
            if para.license:
                referenced_licenses.add(para.license.synopsis)  # type: ignore
        if len(referenced_licenses) == 1 and referenced_licenses != {None}:
            yield UpstreamDatum('License', referenced_licenses.pop(), 'certain')
    else:
        with open(path) as f:
            for line in f:
                m = re.match(r'.* was downloaded from ([^\s]+)', line)
                if m:
                    from_urls.append(m.group(1))

    for from_url in from_urls:
        yield from _metadata_from_url(from_url, origin=path)
        repo_url = guess_repo_from_url(from_url)
        if repo_url:
            yield UpstreamDatum(
                'Repository', repo_url, 'likely')


def url_from_cvs_co_command(command):
    from breezy.location import cvs_to_url
    from breezy import urlutils
    import shlex
    argv = shlex.split(command.decode('utf-8', 'surrogateescape'))
    args = [arg for arg in argv if arg.strip()]
    i = 0
    cvsroot = None
    module = None
    command_seen = False
    del args[0]
    while i < len(args):
        if args[i] == '-d':
            del args[i]
            cvsroot = args[i]
            del args[i]
            continue
        if args[i].startswith('-d'):
            cvsroot = args[i][2:]
            del args[i]
            continue
        if command_seen and not args[i].startswith('-'):
            module = args[i]
        elif args[i] in ('co', 'checkout'):
            command_seen = True
        del args[i]
    if cvsroot is not None:
        url = cvs_to_url(cvsroot)
        if module is not None:
            return urlutils.join(url, module)
        return url
    return None


url_from_svn_co_command = _upstream_ontologist.url_from_svn_co_command
url_from_git_clone_command = _upstream_ontologist.url_from_git_clone_command
url_from_fossil_clone_command = _upstream_ontologist.url_from_fossil_clone_command
guess_from_meson = _upstream_ontologist.guess_from_meson
guess_from_pubspec_yaml = _upstream_ontologist.guess_from_pubspec_yaml


def guess_from_install(path, trust_package):  # noqa: C901
    urls = []
    try:
        with open(path, 'rb') as f:
            lines = list(f.readlines())
            for i, line in enumerate(lines):
                line = line.strip()
                cmdline = line.strip().lstrip(b'$').strip()
                if (cmdline.startswith(b'git clone ')
                        or cmdline.startswith(b'fossil clone ')):
                    while cmdline.endswith(b'\\'):
                        cmdline += lines[i + 1]
                        cmdline = cmdline.strip()
                        i += 1
                    if cmdline.startswith(b'git clone '):
                        url = url_from_git_clone_command(cmdline)
                    elif cmdline.startswith(b'fossil clone '):
                        url = url_from_fossil_clone_command(cmdline)
                    if url:
                        urls.append(url)
                for m in re.findall(b"[\"'`](git clone.*)[\"`']", line):
                    url = url_from_git_clone_command(m)
                    if url:
                        urls.append(url)
                project_re = b'([^/]+)/([^/?.()"#>\\s]*[^-/?.()"#>\\s])'
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
    except IsADirectoryError:
        pass


def guess_from_readme(path, trust_package):  # noqa: C901
    urls = []
    try:
        with open(path, 'rb') as f:
            lines = list(f.readlines())
            for i, line in enumerate(lines):
                line = line.strip()
                cmdline = line.strip().lstrip(b'$').strip()
                if (cmdline.startswith(b'git clone ')
                        or cmdline.startswith(b'fossil clone ')):
                    while cmdline.endswith(b'\\'):
                        cmdline += lines[i + 1]
                        cmdline = cmdline.strip()
                        i += 1
                    if cmdline.startswith(b'git clone '):
                        url = url_from_git_clone_command(cmdline)
                    elif cmdline.startswith(b'fossil clone '):
                        url = url_from_fossil_clone_command(cmdline)
                    if url:
                        urls.append(url)
                for m in re.findall(b"[\"'`](git clone.*)[\"`']", line):
                    url = url_from_git_clone_command(m)
                    if url:
                        urls.append(url)
                m = re.fullmatch(rb'cvs.*-d\s*:pserver:.*', line)
                if m:
                    url = url_from_cvs_co_command(m.group(0))
                    if url:
                        urls.append(url)
                for m in re.finditer(b'($ )?(svn co .*)', line):
                    url = url_from_svn_co_command(m.group(2))
                    if url:
                        urls.append(url)
                project_re = b'([^/]+)/([^/?.()"#>\\s]*[^-,/?.()"#>\\s])'
                for m in re.finditer(
                        b'https://travis-ci.org/' + project_re, line):
                    yield UpstreamDatum(
                        'Repository', 'https://github.com/{}/{}'.format(
                            m.group(1).decode(), m.group(2).decode().rstrip()),
                        'possible')
                for m in re.finditer(
                        b'https://coveralls.io/r/' + project_re, line):
                    yield UpstreamDatum(
                        'Repository', 'https://github.com/{}/{}'.format(
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
                'Description', description, 'possible')
        yield from extra_md
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


guess_from_meta_json = _upstream_ontologist.guess_from_meta_json
guess_from_travis_yml = _upstream_ontologist.guess_from_travis_yml
guess_from_meta_yml = _upstream_ontologist.guess_from_meta_yml
guess_from_metainfo = _upstream_ontologist.guess_from_metainfo
guess_from_doap = _upstream_ontologist.guess_from_doap
guess_from_opam = _upstream_ontologist.guess_from_opam


def guess_from_nuspec(path, trust_package=False):
    # Documentation: https://docs.microsoft.com/en-us/nuget/reference/nuspec

    import xml.etree.ElementTree as ET
    try:
        root = xmlparse_simplify_namespaces(path, [
            "http://schemas.microsoft.com/packaging/2010/07/nuspec.xsd"])
    except ET.ParseError as e:
        logger.warning('Unable to parse nuspec: %s', e)
        return
    assert root.tag == 'package', 'root tag is %r' % root.tag
    metadata = root.find('metadata')
    if metadata is None:
        return
    version_tag = metadata.find('version')
    if version_tag is not None:
        yield UpstreamDatum('Version', version_tag.text, 'certain')
    description_tag = metadata.find('description')
    if description_tag is not None:
        yield UpstreamDatum('Description', description_tag.text, 'certain')
    authors_tag = metadata.find('authors')
    if authors_tag is not None:
        yield UpstreamDatum(
            'Author',
            [Person.from_string(p) for p in authors_tag.text.split(',')],
            'certain')
    project_url_tag = metadata.find('projectUrl')
    if project_url_tag is not None:
        repo_url = guess_repo_from_url(project_url_tag.text)
        if repo_url:
            yield UpstreamDatum('Repository', repo_url, 'confident')
        yield UpstreamDatum('Homepage', project_url_tag.text, 'certain')
    license_tag = metadata.find('license')
    if license_tag is not None:
        yield UpstreamDatum('License', license_tag.text, 'certain')
    copyright_tag = metadata.find('copyright')
    if copyright_tag is not None:
        yield UpstreamDatum('Copyright', copyright_tag.text, 'certain')
    title_tag = metadata.find('title')
    if title_tag is not None:
        yield UpstreamDatum('Name', title_tag.text, 'likely')
    summary_tag = metadata.find('title')
    if summary_tag is not None:
        yield UpstreamDatum('Summary', summary_tag.text, 'certain')
    repository_tag = metadata.find('repository')
    if repository_tag is not None:
        repo_url = repository_tag.get('url')
        branch = repository_tag.get('branch')
        yield UpstreamDatum('Repository', unsplit_vcs_url(repo_url, branch), 'certain')


def guess_from_cabal_lines(lines):  # noqa: C901
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
                yield 'Maintainer', Person.from_string(value)
            if field == 'copyright':
                yield 'Copyright', value
            if field == 'license':
                yield 'License', value
            if field == 'author':
                yield 'Author', Person.from_string(value)
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


def guess_from_cabal(path, trust_package=False):
    with open(path, encoding='utf-8') as f:
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
            elif key == b'PACKAGE_TARNAME':
                yield UpstreamDatum(
                    'Name', value.decode(), 'certain', './configure')
            elif key == b'PACKAGE_VERSION':
                yield UpstreamDatum(
                    'Version', value.decode(), 'certain', './configure')
            elif key == b'PACKAGE_BUGREPORT':
                if value in (b'BUG-REPORT-ADDRESS', ):
                    certainty = 'invalid'
                elif is_email_address(value.decode()):
                    # Downgrade the trustworthiness of this field for most
                    # upstreams if it contains an e-mail address. Most
                    # upstreams seem to just set this to some random address,
                    # and then forget about it.
                    certainty = 'possible'
                elif b'mailing list' in value:
                    # Downgrade the trustworthiness of this field if
                    # it contains a mailing list
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


def guess_from_r_description(path, trust_package: bool = False):  # noqa: C901
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
        if description.get('Version'):
            yield UpstreamDatum('Version', description['Version'], 'certain')
        if 'License' in description:
            yield UpstreamDatum('License', description['License'], 'certain')
        if 'Title' in description:
            yield UpstreamDatum('Summary', description['Title'], 'certain')
        if 'Description' in description:
            lines = description['Description'].splitlines(True)
            if lines:
                reflowed = lines[0] + textwrap.dedent(''.join(lines[1:]))
                yield UpstreamDatum('Description', reflowed, 'certain')
        if 'Maintainer' in description:
            yield UpstreamDatum(
                'Maintainer', Person.from_string(description['Maintainer']), 'certain')
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
        yield UpstreamDatum('Version', m.group(2), 'possible')
    else:
        yield UpstreamDatum('Name', basename, 'possible')


def guess_from_cargo(path, trust_package):
    # see https://doc.rust-lang.org/cargo/reference/manifest.html
    try:
        from tomlkit import loads
        from tomlkit.exceptions import ParseError
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return
    try:
        with open(path) as f:
            cargo = loads(f.read())
    except FileNotFoundError:
        return
    except ParseError as e:
        logger.warning('Error parsing toml file %s: %s', path, e)
        return
    try:
        package = cargo['package']
    except KeyError:
        pass
    else:
        for field, value in package.items():  # type: ignore
            if field == 'name':
                yield UpstreamDatum('Name', str(value), 'certain')
                yield UpstreamDatum('Cargo-Crate', str(value), 'certain')
            elif field == 'description':
                yield UpstreamDatum('Summary', str(value), 'certain')
            elif field == 'homepage':
                yield UpstreamDatum('Homepage', str(value), 'certain')
            elif field == 'license':
                yield UpstreamDatum('License', str(value), 'certain')
            elif field == 'repository':
                yield UpstreamDatum('Repository', str(value), 'certain')
            elif field == 'version':
                yield UpstreamDatum('Version', str(value), 'confident')
            elif field == 'authors':
                yield UpstreamDatum(
                    'Author',
                    [Person.from_string(author) for author in value], 'confident')
            elif field in ('edition', 'default-run'):
                pass
            else:
                logger.debug('Unknown Cargo field %s (%r)', field, value)


def guess_from_pyproject_toml(path, trust_package):
    try:
        from tomlkit import loads
        from tomlkit.exceptions import ParseError
    except ModuleNotFoundError as e:
        warn_missing_dependency(path, e.name)
        return
    try:
        with open(path) as f:
            pyproject = loads(f.read())
    except FileNotFoundError:
        return
    except ParseError as e:
        logger.warning('Error parsing toml file %s: %s', path, e)
        return
    try:
        poetry = pyproject['tool']['poetry']  # type: ignore
    except KeyError:
        pass
    else:
        yield from guess_from_poetry(poetry)


def guess_from_poetry(poetry):
    for key, value in poetry.items():
        if key == 'version':
            yield UpstreamDatum('Version', str(value), 'certain')
        elif key == 'description':
            yield UpstreamDatum('Summary', str(value), 'certain')
        elif key == 'license':
            yield UpstreamDatum('License', str(value), 'certain')
        elif key == 'repository':
            yield UpstreamDatum('Repository', str(value), 'certain')
        elif key == 'name':
            yield UpstreamDatum('Name', str(value), 'certain')
        elif key == 'urls':
            yield from parse_python_project_urls(value)
        elif key == 'keywords':
            yield UpstreamDatum('Keywords', [str(x) for x in value], 'certain')
        elif key == 'authors':
            yield UpstreamDatum('Author', [Person.from_string(x) for x in value], 'certain')
        elif key == 'homepage':
            yield UpstreamDatum('Homepage', str(value), 'certain')
        elif key == 'documentation':
            yield UpstreamDatum('Documentation', str(value), 'certain')
        elif key in ('packages', 'readme', 'classifiers', 'dependencies',
                     'dev-dependencies', 'scripts'):
            pass
        else:
            logger.debug('Unknown field %s (%r) for poetry', key, value)


def guess_from_pom_xml(path, trust_package=False):  # noqa: C901
    # Documentation: https://maven.apache.org/pom.html

    import xml.etree.ElementTree as ET
    try:
        root = xmlparse_simplify_namespaces(path, [
            'http://maven.apache.org/POM/4.0.0'])
    except ET.ParseError as e:
        logger.warning('Unable to parse package.xml: %s', e)
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
        yield UpstreamDatum('Summary', description_tag.text, 'certain')
    version_tag = root.find('version')
    if version_tag is not None and '$' not in version_tag.text:
        yield UpstreamDatum('Version', version_tag.text, 'certain')
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
            if (url_tag.text.startswith('scm:')
                    and url_tag.text.count(':') >= 3):
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
                logger.warning(
                    'Invalid format for SCM connection: %s', connection)
                continue
            if scm != 'scm':
                logger.warning(
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
    if url_tag is not None:
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
                certainty = 'likely' if '$' not in url else 'possible'
                if url:
                    yield UpstreamDatum('Repository', url, certainty)


# https://docs.github.com/en/free-pro-team@latest/github/\
# managing-security-vulnerabilities/adding-a-security-policy-to-your-repository
def guess_from_security_md(name, path, trust_package=False):
    if path.startswith('./'):
        path = path[2:]
    # TODO(jelmer): scan SECURITY.md for email addresses/URLs with instructions
    yield UpstreamDatum('Security-MD', name, 'certain')


def guess_from_go_mod(path, trust_package=False):
    # See https://golang.org/doc/modules/gomod-ref
    with open(path, 'rb') as f:
        for line in f:
            if line.startswith(b'module '):
                modname = line.strip().split(b' ', 1)[1]
                yield UpstreamDatum('Name', modname.decode('utf-8'), 'certain')


def guess_from_gemspec(path, trust_package=False):  # noqa: C901
    # TODO(jelmer): use a proper ruby wrapper instead?
    with open(path) as f:
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

                def parseval(v):
                    v = v.strip()
                    if v.startswith('"') and v.endswith('".freeze'):
                        return v[1:-len('".freeze')]
                    elif v.startswith('"') and v.endswith('"'):
                        return v[1:-1]
                    elif v.startswith("'") and v.endswith("'"):
                        return v[1:-1]
                    elif v.startswith("[") and v.endswith("]"):
                        return [parseval(k) for k in v[1:-1].split(',')]
                    else:
                        raise ValueError
                try:
                    val = parseval(rawval)
                except ValueError:
                    continue

                if key == "name":
                    yield UpstreamDatum('Name', val, 'certain')
                elif key == 'version':
                    yield UpstreamDatum('Version', val, 'certain')
                elif key == 'homepage':
                    yield UpstreamDatum('Homepage', val, 'certain')
                elif key == 'summary':
                    yield UpstreamDatum('Summary', val, 'certain')
                elif key == 'description':
                    yield UpstreamDatum('Description', val, 'certain')
                elif key == 'rubygems_version':
                    pass
                elif key == 'required_ruby_version':
                    pass
                elif key == 'license':
                    yield UpstreamDatum('License', val, 'certain')
                elif key == 'authors':
                    yield UpstreamDatum('Authors', [Person.from_string(a) for a in val], 'certain')
                elif key == 'email':
                    # Should we assume this belongs to the maintainer? mailing list?
                    pass
                elif key == 'require_paths':
                    pass
                elif key in ('rdoc_options', 'extra_rdoc_options'):
                    pass
                else:
                    logger.debug('unknown field %s (%r) in gemspec', key, val)
            else:
                logger.debug(
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
                yield UpstreamDatum('Version', m.group(1).decode(), 'confident')


guess_from_metadata_json = _upstream_ontologist.guess_from_metadata_json


guess_from_authors = _upstream_ontologist.guess_from_authors


def _get_guessers(path, trust_package=False):  # noqa: C901
    CANDIDATES: List[Tuple[str, Callable[[str, bool], Iterator[UpstreamDatum]]]] = [
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
        ('pyproject.toml', guess_from_pyproject_toml),
        ('setup.cfg', guess_from_setup_cfg),
        ('go.mod', guess_from_go_mod),
        ('Makefile.PL', guess_from_makefile_pl),
        ('wscript', guess_from_wscript),
        ('AUTHORS', guess_from_authors),
        ('INSTALL', guess_from_install),
        ('pubspec.yaml', guess_from_pubspec_yaml),
        ('pubspec.yml', guess_from_pubspec_yaml),
        ('meson.build', guess_from_meson),
        ('metadata.json', guess_from_metadata_json),
        ('.travis.yml', guess_from_travis_yml),
    ]

    for name in ('SECURITY.md', '.github/SECURITY.md', 'docs/SECURITY.md'):
        CANDIDATES.append((name, partial(guess_from_security_md, name)))

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
        if n.endswith('.doap')
        or (n.endswith('.xml') and n.startswith('doap_XML_'))]
    if doap_filenames:
        if len(doap_filenames) == 1:
            CANDIDATES.append((doap_filenames[0], guess_from_doap))
        else:
            logger.warning(
                'More than one doap filename, ignoring all: %r',
                doap_filenames)

    metainfo_filenames = [
        n for n in os.listdir(path)
        if n.endswith('.metainfo.xml')]
    if metainfo_filenames:
        if len(metainfo_filenames) == 1:
            CANDIDATES.append((metainfo_filenames[0], guess_from_metainfo))
        else:
            logger.warning(
                'More than one metainfo filename, ignoring all: %r',
                metainfo_filenames)

    cabal_filenames = [n for n in os.listdir(path) if n.endswith('.cabal')]
    if cabal_filenames:
        if len(cabal_filenames) == 1:
            CANDIDATES.append((cabal_filenames[0], guess_from_cabal))
        else:
            logger.warning(
                'More than one cabal filename, ignoring all: %r',
                cabal_filenames)

    readme_filenames = [
        n for n in os.listdir(path)
        if any([n.startswith(p)
                for p in ['readme', 'ReadMe', 'Readme', 'README', 'HACKING', 'CONTRIBUTING']])
        and os.path.splitext(n)[1] not in ('.html', '.pdf', '.xml')
        and not n.endswith('~')]
    CANDIDATES.extend([(n, guess_from_readme) for n in readme_filenames])

    nuspec_filenames = [n for n in os.listdir(path) if n.endswith('.nuspec')]
    if nuspec_filenames:
        if len(nuspec_filenames) == 1:
            CANDIDATES.append((nuspec_filenames[0], guess_from_nuspec))
        else:
            logger.warning(
                'More than one nuspec filename, ignoring all: %r',
                nuspec_filenames)

    opam_filenames = [n for n in os.listdir(path) if n.endswith('.opam')]
    if opam_filenames:
        if len(opam_filenames) == 1:
            CANDIDATES.append((opam_filenames[0], guess_from_opam))
        else:
            logger.warning(
                'More than one opam filename, ignoring all: %r',
                opam_filenames)

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
        yield relpath, guesser(abspath, trust_package)


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


def get_upstream_info(
        path: str, trust_package: bool = False,
        net_access: bool = False, consult_external_directory: bool = False,
        check: bool = False) -> Dict[str, Any]:
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
        metadata_items, path: str,
        net_access: bool = False,
        consult_external_directory: bool = False,
        check: bool = False) -> Dict[str, Any]:
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
    upstream_metadata: UpstreamMetadata = {}
    update_from_guesses(
        upstream_metadata,
        filter_bad_guesses(metadata_items))

    extend_upstream_metadata(
        upstream_metadata, path, net_access=net_access,
        consult_external_directory=consult_external_directory)

    if check:
        check_upstream_metadata(upstream_metadata)

    fix_upstream_metadata(upstream_metadata)

    return {
        k: cast(UpstreamDatum, v).value
        for (k, v) in upstream_metadata.items()}


def guess_upstream_metadata(
        path: str, trust_package: bool = False,
        net_access: bool = False,
        consult_external_directory: bool = False,
        check: bool = False) -> Dict[str, Any]:
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
        from bs4 import BeautifulSoup, Tag
    except ModuleNotFoundError:
        logger.warning(
            'Not scanning sourceforge page, since python3-bs4 is missing')
        return None
    bs = BeautifulSoup(page, features='lxml')
    el = bs.find(id='access_url')
    if el is None or not isinstance(el, Tag):
        return None
    value = el.get('value')
    if value is None:
        return None
    access_command = value.split(' ')  # type: ignore
    if access_command[:2] != ['git', 'clone']:
        return None
    return access_command[2]


def guess_from_sf(sf_project: str, subproject: Optional[str] = None):  # noqa: C901
    try:
        data = get_sf_metadata(sf_project)
    except (socket.timeout, TimeoutError):
        logger.warning(
            'timeout contacting sourceforge, ignoring: %s',
            sf_project)
        return
    except urllib.error.URLError as e:
        logger.warning(
            'Unable to retrieve sourceforge project metadata: %s: %s',
            sf_project, e)
        return

    if data.get('name'):
        yield 'Name', data['name']
    if data.get('external_homepage'):
        yield 'Homepage', data['external_homepage']
    if data.get('preferred_support_url'):
        try:
            canonical_url = check_bug_database_canonical(data['preferred_support_url'])
        except UrlUnverifiable:
            yield 'Bug-Database', data['preferred_support_url']
        except InvalidUrl as e:
            logger.debug(
                'Ignoring invalid preferred_support_url %s: %s',
                e.url, e.reason)
        else:
            yield 'Bug-Database', canonical_url
    # In theory there are screenshots linked from the sourceforge project that
    # we can use, but if there are multiple "subprojects" then it will be
    # unclear which one they belong to.
    # TODO(jelmer): What about cvs and bzr?
    VCS_NAMES = ['hg', 'git', 'svn', 'cvs', 'bzr']
    vcs_tools = [
        (tool['name'], tool.get('mount_label'), tool['url'])
        for tool in data.get('tools', []) if tool['name'] in VCS_NAMES]
    if len(vcs_tools) > 1:
        # Try to filter out some irrelevant stuff
        vcs_tools = [tool for tool in vcs_tools
                     if tool[2].strip('/').rsplit('/')[-1] not in ['www', 'homepage']]
    if len(vcs_tools) > 1 and subproject:
        new_vcs_tools = [
            tool for tool in vcs_tools
            if tool[1] == subproject]
        if len(new_vcs_tools) > 0:
            vcs_tools = new_vcs_tools
    # if both vcs and another tool appear, then assume cvs is old.
    if len(vcs_tools) > 1 and 'cvs' in [t[0] for t in vcs_tools]:
        vcs_tools = [v for v in vcs_tools if v[0] != 'cvs']
    if len(vcs_tools) == 1:
        (kind, _label, url) = vcs_tools[0]
        if kind == 'git':
            url = urljoin('https://sourceforge.net/', url)
            headers = {'User-Agent': USER_AGENT, 'Accept': 'text/html'}
            with urlopen(
                    Request(url, headers=headers),
                    timeout=DEFAULT_URLLIB_TIMEOUT) as resp:
                url = _sf_git_extract_url(resp.read())
        elif kind == 'svn':
            url = urljoin('https://svn.code.sf.net/', url)
        elif kind == 'hg':
            url = urljoin('https://hg.code.sf.net/', url)
        elif kind == 'cvs':
            url = 'cvs+pserver://anonymous@{}.cvs.sourceforge.net/cvsroot/{}'.format(
                sf_project, url.strip('/').rsplit('/')[-2])
        elif kind == 'bzr':
            # TODO(jelmer)
            url = None
        else:
            raise KeyError(kind)
        if url is not None:
            yield 'Repository', url
    elif len(vcs_tools) > 1:
        logger.warning('multiple possible VCS URLs found: %r', vcs_tools)


def guess_from_repology(repology_project):
    try:
        metadata = get_repology_metadata(repology_project)
    except (socket.timeout, TimeoutError):
        logger.warning(
            'timeout contacting repology, ignoring: %s', repology_project)
        return

    fields: Dict[str, Dict[str, int]] = {}

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
                _add_field('License', license, score)

        if 'summary' in entry:
            _add_field('Summary', entry['summary'], score)

        if 'downloads' in entry:
            for download in entry['downloads']:
                _add_field('Download', download, score)

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
    # The set of fields that repology can possibly provide:
    repology_fields = ['Homepage', 'License', 'Summary', 'Download']
    certainty = 'confident'

    if certainty_sufficient(certainty, minimum_certainty):
        # Don't bother talking to repology if we're not
        # speculating.
        return

    return extend_from_external_guesser(
        upstream_metadata, certainty, repology_fields,
        guess_from_repology(source_package))


class NoSuchPackage(Exception):

    def __init__(self, package):
        self.package = package


def guess_from_hackage(hackage_package):
    http_url = 'http://hackage.haskell.org/package/{}/{}.cabal'.format(
        hackage_package, hackage_package)
    headers = {'User-Agent': USER_AGENT}
    try:
        http_contents = urlopen(
            Request(http_url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT).read()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            raise NoSuchPackage(hackage_package) from e
        raise
    return guess_from_cabal_lines(
        http_contents.decode('utf-8', 'surrogateescape').splitlines(True))


class Forge:
    """A Forge."""

    name: str
    repository_browse_can_be_homepage: bool = True

    @classmethod
    def extend_metadata(cls, metadata, project, max_certainty):
        raise NotImplementedError(cls.extend_metadata)

    @classmethod
    def bug_database_url_from_bug_submit_url(cls, parsed_url):
        raise NotImplementedError(cls.bug_database_url_from_bug_submit_url)

    @classmethod
    def check_bug_database_canonical(cls, parsed_url):
        raise NotImplementedError(cls.check_bug_database_canonical)

    @classmethod
    def bug_submit_url_from_bug_database_url(cls, parsed_url):
        raise NotImplementedError(cls.bug_submit_url_from_bug_database_url)

    @classmethod
    def check_bug_submit_url_canonical(cls, url):
        raise NotImplementedError(cls.check_bug_submit_url_canonical)

    @classmethod
    def bug_database_from_issue_url(cls, parsed_url):
        raise NotImplementedError(cls.bug_database_from_issue_url)

    @classmethod
    def repo_url_from_merge_request_url(cls, parsed_url):
        raise NotImplementedError(cls.repo_url_from_merge_request_url)

    @classmethod
    def bug_database_url_from_repo_url(cls, parsed_url):
        raise NotImplementedError(cls.bug_database_url_from_repo_url)


class Launchpad(Forge):

    name = 'Launchpad'

    @classmethod
    def bug_database_url_from_bug_submit_url(cls, parsed_url):
        if parsed_url.netloc != 'bugs.launchpad.net':
            return None
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) >= 1:
            return urlunparse(
                parsed_url._replace(path='/%s' % path_elements[0]))
        return None

    @classmethod
    def bug_submit_url_from_bug_database_url(cls, parsed_url):
        if parsed_url.netloc != 'bugs.launchpad.net':
            return None
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) == 1:
            return urlunparse(
                parsed_url._replace(path=parsed_url.path + '/+filebug'))
        return None


def find_forge(parsed_url) -> Optional[Type[Forge]]:
    if parsed_url.netloc == 'sourceforge.net':
        return SourceForge
    if parsed_url.netloc == 'github.com':
        return GitHub()
    if parsed_url.netloc.endswith('.launchpad.net'):
        return Launchpad
    if is_gitlab_site(parsed_url.netloc):
        return GitLab()
    return None


class PackageRepository:

    name: str

    supported_fields: List[str]

    @classmethod
    def extend_metadata(cls, metadata, name, max_certainty):
        return extend_from_external_guesser(
            metadata, max_certainty, cls.supported_fields,
            cls.guess_metadata(name))

    @classmethod
    def guess_metadata(cls, name):
        raise NotImplementedError(cls.guess_metadata)


class Hackage(PackageRepository):

    name = 'Hackage'

    # The set of fields that sf can possibly provide:
    supported_fields = [
        'Homepage', 'Name', 'Repository', 'Maintainer', 'Copyright',
        'License', 'Bug-Database']

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_hackage(name)


class CratesIo(PackageRepository):

    name = 'crates.io'

    # The set of fields that crates.io can possibly provide:
    supported_fields = [
        'Homepage', 'Name', 'Repository', 'Version', 'Summary']

    @classmethod
    def _parse_crates_io(cls, data):
        crate_data = data['crate']
        yield 'Name', crate_data['name']
        if crate_data.get('homepage'):
            yield 'Homepage', crate_data['homepage']
        if crate_data.get('repository'):
            yield 'Repository', crate_data['repository']
        if crate_data.get('newest_version'):
            yield 'Version', crate_data['newest_version']
        if crate_data.get('description'):
            yield 'Summary', crate_data['description']

    @classmethod
    def guess_metadata(cls, name):
        data = _load_json_url('https://crates.io/api/v1/crates/%s' % name)
        if data:
            return cls._parse_crates_io(data)


GitHub = _upstream_ontologist.GitHub
GitLab = _upstream_ontologist.GitLab


class SourceForge(Forge):

    name = 'SourceForge'

    supported_fields = [
        'Homepage', 'Name', 'Repository', 'Bug-Database']

    @classmethod
    def extend_metadata(cls, metadata, project, max_certainty):
        if 'Name' in metadata:
            subproject = metadata['Name'].value
        else:
            subproject = None

        return extend_from_external_guesser(
            metadata, max_certainty, cls.supported_fields,
            guess_from_sf(project, subproject=subproject))

    @classmethod
    def bug_database_url_from_bug_submit_url(cls, url):
        parsed_url = urlparse(url)
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) < 3:
            return None
        if path_elements[0] != 'p' or path_elements[2] != 'bugs':
            return None
        if len(path_elements) > 3:
            path_elements.pop(-1)
        return urlunparse(
            parsed_url._replace(path='/'.join(path_elements)))


class Pecl(PackageRepository):

    name = 'Pecl'

    supported_fields = ['Homepage', 'Repository', 'Bug-Database']

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_pecl_package(name)


def extend_from_lp(upstream_metadata, minimum_certainty, package,
                   distribution=None, suite=None):
    # The set of fields that Launchpad can possibly provide:
    lp_fields = ['Homepage', 'Repository', 'Name', 'Download']
    lp_certainty = 'possible'

    if certainty_sufficient(lp_certainty, minimum_certainty):
        # Don't bother talking to launchpad if we're not
        # speculating.
        return

    extend_from_external_guesser(
        upstream_metadata, lp_certainty, lp_fields, guess_from_launchpad(
            package, distribution=distribution, suite=suite))


class ThirdPartyRepository:

    supported_fields: List[str]
    max_supported_certainty = 'possible'

    @classmethod
    def extend_metadata(cls, metadata, name, min_certainty):
        if certainty_sufficient(cls.max_supported_certainty, min_certainty):
            # Don't bother if we can't meet minimum certainty
            return

        extend_from_external_guesser(
            metadata, cls.max_supported_certainty, cls.supported_fields,
            cls.guess_metadata(name))

        raise NotImplementedError(cls.extend_metadata)

    @classmethod
    def guess_metadata(cls, name):
        raise NotImplementedError(cls.guess_metadata)


class Aur(ThirdPartyRepository):

    supported_fields = ['Homepage', 'Repository']
    max_supported_certainty = 'possible'

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_aur(name)


class Gobo(ThirdPartyRepository):

    supported_fields = ['Homepage', 'Repository']
    max_supported_certainty = 'possible'

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_gobo(name)


def extract_sf_project_name(url):
    if isinstance(url, list):
        return None
    m = re.match('https?://sourceforge.net/(projects|p)/([^/]+)', url)
    if m:
        return m.group(2)
    m = re.fullmatch('https?://(.*).(sf|sourceforge).(net|io)/.*', url)
    if m:
        return m.group(1)


def repo_url_from_merge_request_url(url):
    parsed_url = urlparse(url)
    forge = find_forge(parsed_url)
    if forge:
        try:
            return forge.repo_url_from_merge_request_url(url)
        except NotImplementedError:
            return None
    return None


def bug_database_from_issue_url(url):
    parsed_url = urlparse(url)
    forge = find_forge(parsed_url)
    if forge:
        try:
            return forge.bug_database_from_issue_url(url)
        except NotImplementedError:
            return None


def guess_bug_database_url_from_repo_url(url):
    parsed_url = urlparse(url)
    forge = find_forge(parsed_url)
    if forge:
        return forge.bug_database_url_from_repo_url(url)
    return None


def bug_database_url_from_bug_submit_url(url):
    parsed_url = urlparse(url)
    forge = find_forge(parsed_url)
    if forge:
        return forge.bug_database_url_from_bug_submit_url(url)
    return None


def bug_submit_url_from_bug_database_url(url):
    parsed_url = urlparse(url)
    forge = find_forge(parsed_url)
    if forge:
        try:
            return forge.bug_submit_url_from_bug_database_url(url)
        except NotImplementedError:
            return None
    return None


def check_bug_database_canonical(url: str) -> str:
    parsed_url = urlparse(url)
    forge = find_forge(parsed_url)
    if forge:
        try:
            forge.check_bug_database_canonical(parsed_url)
        except NotImplementedError:
            raise UrlUnverifiable(
                url, "forge does not support verifying bug database URL")
    raise UrlUnverifiable(url, "unsupported forge")


def check_bug_submit_url_canonical(url: str) -> str:
    parsed_url = urlparse(url)
    forge = find_forge(parsed_url)
    if forge:
        try:
            return forge.check_bug_submit_url_canonical(parsed_url)
        except NotImplementedError:
            raise UrlUnverifiable(
                url, "forge does not support verifying bug submit URL")
    raise UrlUnverifiable(url, "unsupported forge")


def _extrapolate_repository_from_homepage(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Homepage'].value, net_access=net_access)
    if repo:
        yield UpstreamDatum(
            'Repository', repo,
            min_certainty(['likely', upstream_metadata['Homepage'].certainty]))


def _extrapolate_repository_from_download(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Download'].value, net_access=net_access)
    if repo:
        yield UpstreamDatum(
            'Repository', repo,
            min_certainty(
                ['likely', upstream_metadata['Download'].certainty]))


def _extrapolate_repository_from_bug_db(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Bug-Database'].value, net_access=net_access)
    if repo:
        yield UpstreamDatum(
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
            yield UpstreamDatum(
                'Name', name, min_certainty(
                    ['likely', upstream_metadata['Repository'].certainty]))


def _extrapolate_repository_browse_from_repository(
        upstream_metadata, net_access):
    browse_url = browse_url_from_repo_url(
        upstream_metadata['Repository'].value)
    if browse_url:
        yield UpstreamDatum(
            'Repository-Browse', browse_url,
            upstream_metadata['Repository'].certainty)


def _extrapolate_repository_from_repository_browse(
        upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Repository-Browse'].value,
        net_access=net_access)
    if repo:
        yield UpstreamDatum(
            'Repository', repo,
            upstream_metadata['Repository-Browse'].certainty)


def _extrapolate_bug_database_from_repository(
        upstream_metadata, net_access):
    repo_url = upstream_metadata['Repository'].value
    if not isinstance(repo_url, str):
        return
    bug_db_url = guess_bug_database_url_from_repo_url(repo_url)
    if bug_db_url:
        yield UpstreamDatum(
            'Bug-Database', bug_db_url,
            min_certainty(
                ['likely', upstream_metadata['Repository'].certainty]))


def _extrapolate_bug_submit_from_bug_db(
        upstream_metadata, net_access):
    bug_submit_url = bug_submit_url_from_bug_database_url(
        upstream_metadata['Bug-Database'].value)
    if bug_submit_url:
        yield UpstreamDatum(
            'Bug-Submit', bug_submit_url,
            upstream_metadata['Bug-Database'].certainty)


def _extrapolate_bug_db_from_bug_submit(
        upstream_metadata, net_access):
    bug_db_url = bug_database_url_from_bug_submit_url(
        upstream_metadata['Bug-Submit'].value)
    if bug_db_url:
        yield UpstreamDatum(
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
    security_md_path = upstream_metadata['Security-MD']
    security_url = browse_url_from_repo_url(
        repository_url.value, subpath=security_md_path.value)
    if security_url is None:
        return
    yield UpstreamDatum(   # noqa: B901
        'Security-Contact', security_url,
        certainty=min_certainty(
            [repository_url.certainty, security_md_path.certainty]),
        origin=security_md_path.origin)


def _extrapolate_contact_from_maintainer(upstream_metadata, net_access):
    maintainer = upstream_metadata['Maintainer']
    yield UpstreamDatum(
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
    forge = find_forge(parsed)
    if forge and forge.repository_browse_can_be_homepage:
        yield UpstreamDatum('Homepage', browse_url, 'possible')


def _consult_homepage(upstream_metadata, net_access):
    if not net_access:
        return
    from .homepage import guess_from_homepage
    for entry in guess_from_homepage(upstream_metadata['Homepage'].value):
        entry.certainty = min_certainty([
            upstream_metadata['Homepage'].certainty,
            entry.certainty])
        yield entry


EXTRAPOLATE_FNS = [
    (['Homepage'], ['Repository'], _extrapolate_repository_from_homepage),
    (['Repository-Browse'], ['Homepage'],
     _extrapolate_homepage_from_repository_browse),
    (['Bugs-Database'], ['Bug-Database'], _copy_bug_db_field),
    (['Bug-Database'], ['Repository'], _extrapolate_repository_from_bug_db),
    (['Repository'], ['Repository-Browse'],
     _extrapolate_repository_browse_from_repository),
    (['Repository-Browse'], ['Repository'],
     _extrapolate_repository_from_repository_browse),
    (['Repository'], ['Bug-Database'],
     _extrapolate_bug_database_from_repository),
    (['Bug-Database'], ['Bug-Submit'], _extrapolate_bug_submit_from_bug_db),
    (['Bug-Submit'], ['Bug-Database'], _extrapolate_bug_db_from_bug_submit),
    (['Download'], ['Repository'], _extrapolate_repository_from_download),
    (['Repository'], ['Name'], _extrapolate_name_from_repository),
    (['Repository', 'Security-MD'],
     'Security-Contact', _extrapolate_security_contact_from_security_md),
    (['Maintainer'], ['Contact'],
     _extrapolate_contact_from_maintainer),
    (['Homepage'], ['Bug-Database', 'Repository'], _consult_homepage),
]


def extend_upstream_metadata(upstream_metadata,
                             path, minimum_certainty=None,
                             net_access=False,
                             consult_external_directory=False):
    """Extend a set of upstream metadata.
    """
    # TODO(jelmer): Use EXTRAPOLATE_FNS mechanism for this?
    for field in ['Homepage', 'Bug-Database', 'Bug-Submit', 'Repository',
                  'Repository-Browse', 'Download']:
        if field not in upstream_metadata:
            continue
        project = extract_sf_project_name(upstream_metadata[field].value)
        if project:
            certainty = min_certainty(
                ['likely', upstream_metadata[field].certainty])
            upstream_metadata['Archive'] = UpstreamDatum(
                'Archive', 'SourceForge', certainty)
            upstream_metadata['SourceForge-Project'] = UpstreamDatum(
                'SourceForge-Project', project, certainty)
            break

    archive = upstream_metadata.get('Archive')
    if (archive and archive.value == 'SourceForge'
            and 'SourceForge-Project' in upstream_metadata
            and net_access):
        sf_project = upstream_metadata['SourceForge-Project'].value
        sf_certainty = upstream_metadata['Archive'].certainty
        try:
            SourceForge.extend_metadata(
                upstream_metadata, sf_project, sf_certainty)
        except NoSuchForgeProject:
            del upstream_metadata['SourceForge-Project']

    if (archive and archive.value == 'Hackage'
            and 'Hackage-Package' in upstream_metadata
            and net_access):
        hackage_package = upstream_metadata['Hackage-Package'].value
        hackage_certainty = upstream_metadata['Archive'].certainty

        try:
            Hackage.extend_metadata(upstream_metadata, hackage_package, hackage_certainty)
        except NoSuchPackage:
            del upstream_metadata['Hackage-Package']

    if (archive and archive.value == 'crates.io'
            and 'Cargo-Crate' in upstream_metadata
            and net_access):
        crate = upstream_metadata['Cargo-Crate'].value
        crates_io_certainty = upstream_metadata['Archive'].certainty
        try:
            CratesIo.extend_metadata(
                upstream_metadata, crate, crates_io_certainty)
        except NoSuchPackage:
            del upstream_metadata['Cargo-Crate']

    if (archive and archive.value == 'Pecl'
            and 'Pecl-Package' in upstream_metadata
            and net_access):
        pecl_package = upstream_metadata['Pecl-Package'].value
        pecl_certainty = upstream_metadata['Archive'].certainty
        Pecl.extend_metadata(upstream_metadata, pecl_package, pecl_certainty)

    if net_access and consult_external_directory:
        # TODO(jelmer): Don't assume debian/control exists
        from debian.deb822 import Deb822

        try:
            with open(os.path.join(path, 'debian/control')) as f:
                package = Deb822(f)['Source']
        except FileNotFoundError:
            # Huh, okay.
            pass
        else:
            extend_from_lp(upstream_metadata, minimum_certainty, package)
            Aur.extend_metadata(upstream_metadata, package, minimum_certainty)
            Gobo.extend_metadata(upstream_metadata, package, minimum_certainty)
            extend_from_repology(upstream_metadata, minimum_certainty, package)

    _extrapolate_fields(
        upstream_metadata, net_access=net_access,
        minimum_certainty=minimum_certainty)


DEFAULT_ITERATION_LIMIT = 100


def _extrapolate_fields(
        upstream_metadata, net_access: bool = False,
        minimum_certainty: Optional[str] = None,
        iteration_limit: int = DEFAULT_ITERATION_LIMIT):
    changed = True
    iterations = 0
    while changed:
        changed = False
        iterations += 1
        if iterations > iteration_limit:
            raise Exception('hit iteration limit %d' % iteration_limit)
        for from_fields, to_fields, fn in EXTRAPOLATE_FNS:
            from_certainties: Optional[List[str]] = []
            for from_field in from_fields:
                try:
                    from_value = upstream_metadata[from_field]
                except KeyError:
                    from_certainties = None
                    break
                from_certainties.append(from_value.certainty)  # type: ignore
            if not from_certainties:
                # Nope
                continue
            from_certainty = min_certainty(from_certainties)
            old_to_values = {
                to_field: upstream_metadata.get(to_field)
                for to_field in to_fields}
            if all([old_value is not None
                    and certainty_to_confidence(from_certainty) > certainty_to_confidence(old_value.certainty)  # type: ignore
                    for old_value in old_to_values.values()]):
                continue
            changes = update_from_guesses(upstream_metadata, fn(upstream_metadata, net_access))
            if changes:
                logger.debug(
                    'Extrapolating (%r ⇒ %r) from (\'%s: %s\', %s)',
                    ["{}: {}".format(us.field, us.value) for us in old_to_values.values() if us],
                    ["{}: {}".format(us.field, us.value) for us in changes if us],
                    from_value.field, from_value.value, from_value.certainty)
                changed = True


def verify_screenshots(urls: List[str]) -> Iterator[Tuple[str, Optional[bool]]]:
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


def check_url_canonical(url: str) -> str:
    parsed_url = urlparse(url)
    if parsed_url.scheme not in ('https', 'http'):
        raise UrlUnverifiable(
            url, "unable to check URL with scheme %s" % parsed_url.scheme)
    headers = {'User-Agent': USER_AGENT}
    try:
        resp = urlopen(
            Request(url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.code == 404:
            raise InvalidUrl(url, "url not found") from e
        if e.code == 429:
            raise UrlUnverifiable(url, "rate-by") from e
        if e.code == 503:
            raise UrlUnverifiable(url, "server-down") from e
        raise
    except (socket.timeout, TimeoutError) as e:
        raise UrlUnverifiable(url, 'timeout contacting') from e
    else:
        return resp.geturl()


def check_upstream_metadata(  # noqa: C901
        upstream_metadata: UpstreamMetadata,
        version: Optional[str] = None):  # noqa: C901
    """Check upstream metadata.

    This will make network connections, etc.
    """
    repository = upstream_metadata.get('Repository')
    if repository:
        try:
            canonical_url = check_repository_url_canonical(
                repository.value, version=version)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug(
                'Deleting invalid Repository URL %s: %s',
                e.url,
                e.reason)
            del upstream_metadata["Repository"]
        else:
            repository.value = canonical_url
            if repository.certainty == 'confident':
                repository.certainty = 'certain'
            derived_browse_url = browse_url_from_repo_url(repository.value)
            browse_repo = upstream_metadata.get('Repository-Browse')
            if browse_repo and derived_browse_url == browse_repo.value:
                browse_repo.certainty = repository.certainty
    homepage = upstream_metadata.get('Homepage')
    if homepage:
        try:
            canonical_url = check_url_canonical(homepage.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug(
                'Deleting invalid Homepage URL %s: %s',
                e.url, e.reason)
            del upstream_metadata["Homepage"]
        else:
            homepage.value = canonical_url
            if certainty_sufficient(homepage.certainty, 'likely'):
                homepage.certainty = 'certain'
    repository_browse = upstream_metadata.get('Repository-Browse')
    if repository_browse:
        try:
            canonical_url = check_url_canonical(repository_browse.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug(
                'Deleting invalid Repository-Browse URL %s: %s',
                e.url, e.reason)
            del upstream_metadata['Repository-Browse']
        else:
            repository_browse.value = canonical_url
            if certainty_sufficient(repository_browse.certainty, 'likely'):
                repository_browse.certainty = 'certain'
    bug_database = upstream_metadata.get('Bug-Database')
    if bug_database:
        try:
            canonical_url = check_bug_database_canonical(bug_database.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug("Deleting invalid Bug-Database URL %s: %s",
                         e.url, e.reason)
            del upstream_metadata['Bug-Database']
        else:
            bug_database.value = canonical_url
            if certainty_sufficient(bug_database.certainty, 'likely'):
                bug_database.certainty = 'certain'
    bug_submit = upstream_metadata.get('Bug-Submit')
    if bug_submit:
        try:
            canonical_url = check_bug_submit_url_canonical(bug_submit.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug(
                'Deleting invalid Bug-Submit URL %s: %s',
                e.url, e.reason)
            del upstream_metadata['Bug-Submit']
        else:
            bug_submit.value = canonical_url
            if certainty_sufficient(bug_submit.certainty, 'likely'):
                bug_submit.certainty = 'certain'
    screenshots = upstream_metadata.get('Screenshots')
    if screenshots and screenshots.certainty == 'likely':
        newvalue = []
        screenshots.certainty = 'certain'
        for url, status in verify_screenshots(screenshots.value):
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
        if (line.startswith(b'\t') or line.startswith(b' ')
                or line.startswith(b'#')):
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


def guess_from_pecl_package(package):
    url = 'https://pecl.php.net/packages/%s' % package
    headers = {'User-Agent': USER_AGENT}
    try:
        f = urlopen(
            Request(url, headers=headers),
            timeout=PECL_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        return
    except (socket.timeout, TimeoutError):
        logger.warning('timeout contacting pecl, ignoring: %s', url)
        return
    try:
        from bs4 import BeautifulSoup, Tag
    except ModuleNotFoundError:
        logger.warning(
            'bs4 missing so unable to scan pecl page, ignoring %s', url)
        return
    bs = BeautifulSoup(f.read(), features='lxml')
    tag = bs.find('a', text='Browse Source')
    if isinstance(tag, Tag):
        yield 'Repository-Browse', tag.attrs['href']
    tag = bs.find('a', text='Package Bugs')
    if isinstance(tag, Tag):
        yield 'Bug-Database', tag.attrs['href']
    label_tag = bs.find('th', text='Homepage')
    if isinstance(label_tag, Tag) and label_tag.parent is not None:
        tag = label_tag.parent.find('a')
        if isinstance(tag, Tag):
            yield 'Homepage', tag.attrs['href']


def strip_vcs_prefixes(url):
    for prefix in ['git', 'hg']:
        if url.startswith(prefix + '+'):
            return url[len(prefix) + 1:]
    return url


def guess_from_gobo(package: str):   # noqa: C901
    packages_url = "https://api.github.com/repos/gobolinux/Recipes/contents"
    try:
        contents = _load_json_url(packages_url)
    except urllib.error.HTTPError as e:
        if e.code == 403:
            logger.debug('error loading %s: %r. rate limiting?', packages_url, e)
            return
        raise
    packages = [entry['name'] for entry in contents]
    for p in packages:
        if p.lower() == package.lower():
            package = p
            break
    else:
        logger.debug('No gobo package named %s', package)
        return

    contents_url = "https://api.github.com/repos/gobolinux/Recipes/contents/%s" % package
    try:
        contents = _load_json_url(contents_url)
    except urllib.error.HTTPError as e:
        if e.code == 403:
            logger.debug('error loading %s: %r. rate limiting?', contents_url, e)
            return
        raise
    versions = [entry['name'] for entry in contents]
    base_url = 'https://raw.githubusercontent.com/gobolinux/Recipes/master/{}/{}'.format(package, versions[-1])
    headers = {'User-Agent': USER_AGENT}
    try:
        f = urlopen(
            Request(base_url + '/Recipe', headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.code == 403:
            logger.debug('error loading %s: %r. rate limiting?', base_url, e)
            return
        if e.code != 404:
            raise
    else:
        for line in f:
            m = re.match(b'url="(.*)"$', line)
            if m:
                yield 'Download', m.group(1).decode()

    url = base_url + '/Resources/Description'
    try:
        f = urlopen(
            Request(url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.code == 403:
            logger.debug('error loading %s: %r. rate limiting?', url, e)
            return
        if e.code != 404:
            raise
    else:
        for line in f:
            m = re.match(b'\\[(.*)\\] (.*)', line)
            if not m:
                continue
            key = m.group(1).decode()
            value = m.group(2).decode()
            if key == 'Name':
                yield 'Name', value
            elif key == 'Summary':
                yield 'Summary', value
            elif key == 'License':
                yield 'License', value
            elif key == 'Description':
                yield 'Description', value
            elif key == 'Homepage':
                yield 'Homepage', value
            else:
                logger.warning('Unknown field %s in gobo Description', key)


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
        except (socket.timeout, TimeoutError):
            logger.warning('timeout contacting aur, ignoring: %s', url)
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
            if any([url.startswith(vcs + '+') for vcs in vcses]):
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
                logger.warning('%s', str(e))
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
    except (socket.timeout, TimeoutError):
        logger.warning('timeout contacting launchpad, ignoring')
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
        yield ('SourceForge-Project', project_data['sourceforge_project'])
    if project_data.get('wiki_url'):
        yield ('Wiki', project_data['wiki_url'])
    if project_data.get('summary'):
        yield ('Summary', project_data['summary'])
    if project_data.get('download_url'):
        yield ('Download', project_data['download_url'])
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


def fix_upstream_metadata(upstream_metadata: UpstreamMetadata):
    """Fix existing upstream metadata."""
    if 'Repository' in upstream_metadata:
        repo = upstream_metadata['Repository']
        url = repo.value
        url = sanitize_vcs_url(url)
        repo.value = url
    if 'Summary' in upstream_metadata:
        summary = upstream_metadata['Summary']
        summary.value = summary.value.split('. ')[0]
        summary.value = summary.value.rstrip().rstrip('.')
