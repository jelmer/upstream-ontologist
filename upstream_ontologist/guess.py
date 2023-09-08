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
from typing import Optional, Iterable, List, Iterator, Any, Dict, Tuple, cast, Callable
from urllib.parse import urlparse
from urllib.request import urlopen, Request

from . import _upstream_ontologist

from .vcs import (
    browse_url_from_repo_url,
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


get_sf_metadata = _upstream_ontologist.get_sf_metadata
NoSuchForgeProject = _upstream_ontologist.NoSuchForgeProject
NoSuchRepologyProject = _upstream_ontologist.NoSuchRepologyProject
get_repology_metadata = _upstream_ontologist.get_repology_metadata


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
    'Demo': str,

    # We should possibly hide these:
    'Debian-ITP': int,
}


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
    return _upstream_ontologist.known_bad_guess(datum)


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


extract_pecl_package_name = _upstream_ontologist.extract_pecl_package_name
_metadata_from_url = _upstream_ontologist.metadata_from_url


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
extract_sf_project_name = _upstream_ontologist.extract_sf_project_name
guess_from_pkg_info = _upstream_ontologist.guess_from_pkg_info
guess_from_composer_json = _upstream_ontologist.guess_from_composer_json
guess_from_package_json = _upstream_ontologist.guess_from_package_json
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
                            repo_url = guess_repo_from_url(url)  # type: ignore
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
                            repo_url = guess_repo_from_url(url)  # type: ignore
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


guess_from_debian_patch = _upstream_ontologist.guess_from_debian_patch
guess_from_meta_json = _upstream_ontologist.guess_from_meta_json
guess_from_travis_yml = _upstream_ontologist.guess_from_travis_yml
guess_from_meta_yml = _upstream_ontologist.guess_from_meta_yml
guess_from_metainfo = _upstream_ontologist.guess_from_metainfo
guess_from_doap = _upstream_ontologist.guess_from_doap
guess_from_opam = _upstream_ontologist.guess_from_opam
guess_from_nuspec = _upstream_ontologist.guess_from_nuspec
guess_from_cabal = _upstream_ontologist.guess_from_cabal
guess_from_cabal_lines = _upstream_ontologist.guess_from_cabal_lines
guess_from_configure = _upstream_ontologist.guess_from_configure
guess_from_r_description = _upstream_ontologist.guess_from_r_description
guess_from_environment = _upstream_ontologist.guess_from_environment
guess_from_path = _upstream_ontologist.guess_from_path
guess_from_cargo = _upstream_ontologist.guess_from_cargo
guess_from_pyproject_toml = _upstream_ontologist.guess_from_pyproject_toml
guess_from_setup_cfg = _upstream_ontologist.guess_from_setup_cfg
guess_from_setup_py = _upstream_ontologist.guess_from_setup_py

guess_from_pom_xml = _upstream_ontologist.guess_from_pom_xml
guess_from_git_config = _upstream_ontologist.guess_from_git_config


def guess_from_get_orig_source(path, trust_package=False):
    with open(path, 'rb') as f:
        for line in f:
            if line.startswith(b'git clone'):
                url = url_from_git_clone_command(line)
                if url:
                    certainty = 'likely' if '$' not in url else 'possible'
                    yield UpstreamDatum('Repository', url, certainty)


guess_from_security_md = _upstream_ontologist.guess_from_security_md
guess_from_go_mod = _upstream_ontologist.guess_from_go_mod
guess_from_gemspec = _upstream_ontologist.guess_from_gemspec
guess_from_makefile_pl = _upstream_ontologist.guess_from_makefile_pl
guess_from_wscript = _upstream_ontologist.guess_from_wscript
guess_from_metadata_json = _upstream_ontologist.guess_from_metadata_json
guess_from_authors = _upstream_ontologist.guess_from_authors
guess_from_package_yaml = _upstream_ontologist.guess_from_package_yaml


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
        ('package.yaml', guess_from_package_yaml),
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
    yield 'path', guess_from_path(path, trust_package)

    for relpath, guesser in CANDIDATES:
        abspath = os.path.join(path, relpath)
        if not os.path.exists(abspath):
            continue
        try:
            yield relpath, guesser(abspath, trust_package)
        except _upstream_ontologist.ParseError as err:
            logging.debug('Parse Error in %s: %s', relpath, err)


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
SourceForge = _upstream_ontologist.SourceForge
Launchpad = _upstream_ontologist.Launchpad


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


find_forge = _upstream_ontologist.find_forge
repo_url_from_merge_request_url = _upstream_ontologist.repo_url_from_merge_request_url
bug_database_from_issue_url = _upstream_ontologist.bug_database_from_issue_url
guess_bug_database_url_from_repo_url = _upstream_ontologist.guess_bug_database_url_from_repo_url
bug_database_url_from_bug_submit_url = _upstream_ontologist.bug_database_url_from_bug_submit_url
bug_submit_url_from_bug_database_url = _upstream_ontologist.bug_submit_url_from_bug_database_url
check_bug_database_canonical = _upstream_ontologist.check_bug_database_canonical
check_bug_submit_url_canonical = _upstream_ontologist.check_bug_submit_url_canonical


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
    # Some hosting sites are commonly used as Homepage
    # TODO(jelmer): Maybe check that there is a README file that
    # can serve as index?
    forge = find_forge(browse_url)
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


check_url_canonical = _upstream_ontologist.check_url_canonical


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


guess_from_aur = _upstream_ontologist.guess_from_aur
guess_from_launchpad = _upstream_ontologist.guess_from_launchpad


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
