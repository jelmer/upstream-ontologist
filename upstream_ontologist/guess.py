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

import logging
import os
import re
import socket
import urllib.error
from typing import Any, Dict, Iterable, Iterator, List, Optional, Tuple, cast
from urllib.parse import urlparse
from urllib.request import Request, urlopen

from . import (
    DEFAULT_URLLIB_TIMEOUT,
    USER_AGENT,
    InvalidUrl,
    Person,
    UpstreamDatum,
    UpstreamMetadata,
    UrlUnverifiable,
    _load_json_url,
    _upstream_ontologist,
    certainty_sufficient,
    certainty_to_confidence,
    min_certainty,
)
from .vcs import (
    browse_url_from_repo_url,
    check_repository_url_canonical,
    guess_repo_from_url,
    is_gitlab_site,
)
from .vcs import (
    sanitize_url as sanitize_vcs_url,
)

# Pecl is quite slow, so up the timeout a bit.
PECL_URLLIB_TIMEOUT = 15


logger = logging.getLogger(__name__)


get_sf_metadata = _upstream_ontologist.get_sf_metadata
NoSuchForgeProject = _upstream_ontologist.NoSuchForgeProject
NoSuchRepologyProject = _upstream_ontologist.NoSuchRepologyProject
get_repology_metadata = _upstream_ontologist.get_repology_metadata
guess_from_pod = _upstream_ontologist.guess_from_pod


def guess_upstream_info(path, trust_package):
    return iter(_upstream_ontologist.guess_upstream_info(path, trust_package))


DATUM_TYPES = {
    "Bug-Submit": str,
    "Bug-Database": str,
    "Repository": str,
    "Repository-Browse": str,
    "Documentation": str,
    "Keywords": list,
    "License": str,
    "Go-Import-Path": str,
    "Summary": str,
    "Description": str,
    "Wiki": str,
    "SourceForge-Project": str,
    "Archive": str,
    "Homepage": str,
    "Name": str,
    "Version": str,
    "Download": str,
    "Pecl-Package": str,
    "Screenshots": list,
    "Contact": str,
    "Author": list,
    "Security-MD": str,
    # TODO(jelmer): Allow multiple maintainers?
    "Maintainer": Person,
    "Cargo-Crate": str,
    "API-Documentation": str,
    "Funding": str,
    "GitHub-Project": str,
    "Demo": str,
    # We should possibly hide these:
    "Debian-ITP": int,
}


def known_bad_guess(datum: UpstreamDatum) -> bool:  # noqa: C901
    try:
        expected_type = DATUM_TYPES[datum.field]
    except KeyError:
        logger.warning("Unknown field %s", datum.field)
        return False
    if not isinstance(datum.value, expected_type):
        logger.warning("filtering out bad value %r for %s", datum.value, datum.field)
        return True
    return _upstream_ontologist.known_bad_guess(datum)


def filter_bad_guesses(
    guessed_items: Iterable[UpstreamDatum]
) -> Iterator[UpstreamDatum]:
    for item in guessed_items:
        if known_bad_guess(item):
            logger.debug("Excluding known bad item %r", item)
        else:
            yield item


def update_from_guesses(
    upstream_metadata: UpstreamMetadata, guessed_items: Iterable[UpstreamDatum]
):
    changed = []
    for datum in guessed_items:
        current_datum: Optional[UpstreamDatum] = cast(
            Optional[UpstreamDatum], upstream_metadata.get(datum.field)
        )
        if (
            current_datum is None
            or current_datum.certainty is None
            or (
                datum.certainty is not None
                and certainty_to_confidence(datum.certainty)  # type: ignore
                < certainty_to_confidence(current_datum.certainty)
            )
        ):  # type: ignore
            upstream_metadata[datum.field] = datum  # type: ignore
            changed.append(datum)
    return changed


extract_pecl_package_name = _upstream_ontologist.extract_pecl_package_name


debian_is_native = _upstream_ontologist.debian_is_native


metadata_from_itp_bug_body = _upstream_ontologist.metadata_from_itp_bug_body
extract_sf_project_name = _upstream_ontologist.extract_sf_project_name

url_from_cvs_co_command = _upstream_ontologist.url_from_cvs_co_command
url_from_svn_co_command = _upstream_ontologist.url_from_svn_co_command
url_from_git_clone_command = _upstream_ontologist.url_from_git_clone_command
url_from_fossil_clone_command = _upstream_ontologist.url_from_fossil_clone_command
url_from_vcs_command = _upstream_ontologist.url_from_vcs_command


def guess_from_readme(path, trust_package):  # noqa: C901
    urls = []
    try:
        with open(path, "rb") as f:
            lines = list(f.readlines())
            for i, line in enumerate(lines):
                line = line.strip()
                cmdline = line.strip().lstrip(b"$").strip()
                if (
                    cmdline.startswith(b"git clone ")
                    or cmdline.startswith(b"fossil clone ")
                    or cmdline.startswith(b"hg clone ")
                    or cmdline.startswith(b"bzr co ")
                    or cmdline.startswith(b"bzr branch ")
                ):
                    while cmdline.endswith(b"\\"):
                        cmdline += lines[i + 1]
                        cmdline = cmdline.strip()
                        i += 1
                    url = url_from_vcs_command(cmdline)
                    if url:
                        urls.append(url)
                for m in re.findall(b"[\"'`](git clone.*)[\"`']", line):
                    url = url_from_git_clone_command(m)
                    if url:
                        urls.append(url)
                m = re.fullmatch(rb"cvs.*-d\s*:pserver:.*", line)
                if m:
                    url = url_from_cvs_co_command(m.group(0))
                    if url:
                        urls.append(url)
                for m in re.finditer(b"($ )?(svn co .*)", line):
                    url = url_from_svn_co_command(m.group(2))
                    if url:
                        urls.append(url)
                project_re = b'([^/]+)/([^/?.()"#>\\s]*[^-,/?.()"#>\\s])'
                for m in re.finditer(b"https://travis-ci.org/" + project_re, line):
                    yield UpstreamDatum(
                        "Repository",
                        "https://github.com/{}/{}".format(
                            m.group(1).decode(), m.group(2).decode().rstrip()
                        ),
                        certainty="possible",
                    )
                for m in re.finditer(b"https://coveralls.io/r/" + project_re, line):
                    yield UpstreamDatum(
                        "Repository",
                        "https://github.com/{}/{}".format(
                            m.group(1).decode(), m.group(2).decode().rstrip()
                        ),
                        certainty="possible",
                    )
                for m in re.finditer(
                    b"https://github.com/([^/]+)/([^/]+)/issues", line
                ):
                    yield UpstreamDatum(
                        "Bug-Database", m.group(0).decode().rstrip(), certainty="possible"
                    )
                for m in re.finditer(
                    b"https://github.com/" + project_re + b"(.git)?", line
                ):
                    yield UpstreamDatum(
                        "Repository",
                        m.group(0).rstrip(b".").decode().rstrip(),
                        certainty="possible",
                    )
                m = re.fullmatch(b"https://github.com/" + project_re, line)
                if m:
                    yield UpstreamDatum(
                        "Repository", line.strip().rstrip(b".").decode(), certainty="possible"
                    )
                m = re.fullmatch(b"git://([^ ]+)", line)
                if m:
                    yield UpstreamDatum(
                        "Repository", line.strip().rstrip(b".").decode(), certainty="possible"
                    )
                for m in re.finditer(b'https://([^]/]+)/([^]\\s()"#]+)', line):
                    if is_gitlab_site(m.group(1).decode()):
                        url = m.group(0).rstrip(b".").decode().rstrip()
                        try:
                            repo_url = guess_repo_from_url(url)  # type: ignore
                        except ValueError:
                            logger.warning("Ignoring invalid URL %s in %s", url, path)
                        else:
                            if repo_url:
                                yield UpstreamDatum("Repository", repo_url, certainty="possible")
        if path.lower().endswith("readme.md"):
            with open(path, "rb") as f:
                from .readme import description_from_readme_md

                contents = f.read().decode("utf-8", "surrogateescape")
                description, extra_md = description_from_readme_md(contents)
        elif path.lower().endswith("readme.rst"):
            with open(path, "rb") as f:
                from .readme import description_from_readme_rst

                contents = f.read().decode("utf-8", "surrogateescape")
                description, extra_md = description_from_readme_rst(contents)
        elif path.lower().endswith("readme"):
            with open(path, "rb") as f:
                from .readme import description_from_readme_plain

                contents = f.read().decode("utf-8", "surrogateescape")
                description, extra_md = description_from_readme_plain(contents)
        else:
            description = None
            extra_md = []
        if description is not None:
            yield UpstreamDatum("Description", description, certainty="possible")
        yield from extra_md
        if path.lower().endswith("readme.pod"):
            with open(path, "rb") as f:
                yield from guess_from_pod(f.read())
    except IsADirectoryError:
        pass

    def prefer_public(url):
        parsed_url = urlparse(url)
        if "ssh" in parsed_url.scheme:
            return 1
        return 0

    urls.sort(key=prefer_public)
    if urls:
        yield UpstreamDatum("Repository", urls[0], certainty="possible")


def guess_upstream_metadata_items(
    path: str, trust_package: bool = False, minimum_certainty: Optional[str] = None
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


def get_upstream_info(
    path: str,
    trust_package: bool = False,
    net_access: bool = False,
    consult_external_directory: bool = False,
    check: bool = False,
) -> Dict[str, Any]:
    metadata_items = []
    for entry in guess_upstream_info(path, trust_package=trust_package):
        if isinstance(entry, UpstreamDatum):
            metadata_items.append(entry)
    metadata = summarize_upstream_metadata(
        metadata_items,
        path,
        net_access=net_access,
        consult_external_directory=consult_external_directory,
        check=check,
    )
    return metadata


def summarize_upstream_metadata(
    metadata_items,
    path: str,
    net_access: bool = False,
    consult_external_directory: bool = False,
    check: bool = False,
) -> Dict[str, Any]:
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
    update_from_guesses(upstream_metadata, filter_bad_guesses(metadata_items))

    extend_upstream_metadata(
        upstream_metadata,
        path,
        net_access=net_access,
        consult_external_directory=consult_external_directory,
    )

    if check:
        check_upstream_metadata(upstream_metadata)

    fix_upstream_metadata(upstream_metadata)

    return {k: cast(UpstreamDatum, v).value for (k, v) in upstream_metadata.items()}


def guess_upstream_metadata(
    path: str,
    trust_package: bool = False,
    net_access: bool = False,
    consult_external_directory: bool = False,
    check: bool = False,
) -> Dict[str, Any]:
    """Guess the upstream metadata dictionary.

    Args:
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
      net_access: Whether to allow net access
      consult_external_directory: Whether to pull in data
        from external (user-maintained) directories.
    """
    metadata_items = guess_upstream_metadata_items(path, trust_package=trust_package)
    return summarize_upstream_metadata(
        metadata_items,
        path,
        net_access=net_access,
        consult_external_directory=consult_external_directory,
        check=check,
    )


def _possible_fields_missing(upstream_metadata, fields, field_certainty):
    for field in fields:
        if field not in upstream_metadata:
            return True
        if upstream_metadata[field].certainty != "certain":
            return True
    else:
        return False


def extend_from_external_guesser(
    upstream_metadata, guesser_certainty, guesser_fields, guesser
):
    if not _possible_fields_missing(
        upstream_metadata, guesser_fields, guesser_certainty
    ):
        return

    update_from_guesses(
        upstream_metadata,
        [UpstreamDatum(key, value, certainty=guesser_certainty)
         for (key, value) in guesser],
    )


def extend_from_repology(upstream_metadata, minimum_certainty, source_package):
    # The set of fields that repology can possibly provide:
    repology_fields = ["Homepage", "License", "Summary", "Download"]
    certainty = "confident"

    if certainty_sufficient(certainty, minimum_certainty):
        # Don't bother talking to repology if we're not
        # speculating.
        return

    return extend_from_external_guesser(
        upstream_metadata,
        certainty,
        repology_fields,
        guess_from_repology(source_package),
    )


class NoSuchPackage(Exception):
    def __init__(self, package):
        self.package = package


guess_from_hackage = _upstream_ontologist.guess_from_hackage


class PackageRepository:
    name: str

    supported_fields: List[str]

    @classmethod
    def extend_metadata(cls, metadata, name, max_certainty):
        return extend_from_external_guesser(
            metadata, max_certainty, cls.supported_fields, cls.guess_metadata(name)
        )

    @classmethod
    def guess_metadata(cls, name):
        raise NotImplementedError(cls.guess_metadata)


class Hackage(PackageRepository):
    name = "Hackage"

    # The set of fields that sf can possibly provide:
    supported_fields = [
        "Homepage",
        "Name",
        "Repository",
        "Maintainer",
        "Copyright",
        "License",
        "Bug-Database",
    ]

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_hackage(name)


class CratesIo(PackageRepository):
    name = "crates.io"

    # The set of fields that crates.io can possibly provide:
    supported_fields = ["Homepage", "Name", "Repository", "Version", "Summary"]

    @classmethod
    def _parse_crates_io(cls, data):
        crate_data = data["crate"]
        yield "Name", crate_data["name"]
        if crate_data.get("homepage"):
            yield "Homepage", crate_data["homepage"]
        if crate_data.get("repository"):
            yield "Repository", crate_data["repository"]
        if crate_data.get("newest_version"):
            yield "Version", crate_data["newest_version"]
        if crate_data.get("description"):
            yield "Summary", crate_data["description"]

    @classmethod
    def guess_metadata(cls, name):
        data = _load_json_url("https://crates.io/api/v1/crates/%s" % name)
        if data:
            return cls._parse_crates_io(data)


GitHub = _upstream_ontologist.GitHub
GitLab = _upstream_ontologist.GitLab
SourceForge = _upstream_ontologist.SourceForge
Launchpad = _upstream_ontologist.Launchpad

guess_from_repology = _upstream_ontologist.guess_from_repology


class Pecl(PackageRepository):
    name = "Pecl"

    supported_fields = ["Homepage", "Repository", "Bug-Database"]

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_pecl_package(name)


def extend_from_lp(
    upstream_metadata, minimum_certainty, package, distribution=None, suite=None
):
    # The set of fields that Launchpad can possibly provide:
    lp_fields = ["Homepage", "Repository", "Name", "Download"]
    lp_certainty = "possible"

    if certainty_sufficient(lp_certainty, minimum_certainty):
        # Don't bother talking to launchpad if we're not
        # speculating.
        return

    extend_from_external_guesser(
        upstream_metadata,
        lp_certainty,
        lp_fields,
        guess_from_launchpad(package, distribution=distribution, suite=suite),
    )


class ThirdPartyRepository:
    supported_fields: List[str]
    max_supported_certainty = "possible"

    @classmethod
    def extend_metadata(cls, metadata, name, min_certainty):
        if certainty_sufficient(cls.max_supported_certainty, min_certainty):
            # Don't bother if we can't meet minimum certainty
            return

        extend_from_external_guesser(
            metadata,
            cls.max_supported_certainty,
            cls.supported_fields,
            cls.guess_metadata(name),
        )

        raise NotImplementedError(cls.extend_metadata)

    @classmethod
    def guess_metadata(cls, name):
        raise NotImplementedError(cls.guess_metadata)


class Aur(ThirdPartyRepository):
    supported_fields = ["Homepage", "Repository"]
    max_supported_certainty = "possible"

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_aur(name)


class Gobo(ThirdPartyRepository):
    supported_fields = ["Homepage", "Repository"]
    max_supported_certainty = "possible"

    @classmethod
    def guess_metadata(cls, name):
        return guess_from_gobo(name)


find_forge = _upstream_ontologist.find_forge
repo_url_from_merge_request_url = _upstream_ontologist.repo_url_from_merge_request_url
bug_database_from_issue_url = _upstream_ontologist.bug_database_from_issue_url
guess_bug_database_url_from_repo_url = (
    _upstream_ontologist.guess_bug_database_url_from_repo_url
)
bug_database_url_from_bug_submit_url = (
    _upstream_ontologist.bug_database_url_from_bug_submit_url
)
bug_submit_url_from_bug_database_url = (
    _upstream_ontologist.bug_submit_url_from_bug_database_url
)
check_bug_database_canonical = _upstream_ontologist.check_bug_database_canonical
check_bug_submit_url_canonical = _upstream_ontologist.check_bug_submit_url_canonical


def _extrapolate_repository_from_homepage(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata["Homepage"].value, net_access=net_access
    )
    if repo:
        yield UpstreamDatum(
            "Repository",
            repo,
            certainty=min_certainty(["likely", upstream_metadata["Homepage"].certainty]),
        )


def _extrapolate_repository_from_download(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata["Download"].value, net_access=net_access
    )
    if repo:
        yield UpstreamDatum(
            "Repository",
            repo,
            certainty=min_certainty(["likely", upstream_metadata["Download"].certainty]),
        )


def _extrapolate_repository_from_bug_db(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata["Bug-Database"].value, net_access=net_access
    )
    if repo:
        yield UpstreamDatum(
            "Repository",
            repo,
            certainty=min_certainty(["likely", upstream_metadata["Bug-Database"].certainty]),
        )


def _extrapolate_name_from_repository(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata["Repository"].value, net_access=net_access
    )
    if repo:
        parsed = urlparse(repo)
        name = parsed.path.split("/")[-1]
        if name.endswith(".git"):
            name = name[:-4]
        if name:
            yield UpstreamDatum(
                "Name",
                name,
                certainty=min_certainty(["likely", upstream_metadata["Repository"].certainty]),
            )


def _extrapolate_repository_browse_from_repository(upstream_metadata, net_access):
    browse_url = browse_url_from_repo_url(upstream_metadata["Repository"].value)
    if browse_url:
        yield UpstreamDatum(
            "Repository-Browse", browse_url, certainty=upstream_metadata["Repository"].certainty
        )


def _extrapolate_repository_from_repository_browse(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata["Repository-Browse"].value, net_access=net_access
    )
    if repo:
        yield UpstreamDatum(
            "Repository", repo, certainty=upstream_metadata["Repository-Browse"].certainty
        )


def _extrapolate_bug_database_from_repository(upstream_metadata, net_access):
    repo_url = upstream_metadata["Repository"].value
    if not isinstance(repo_url, str):
        return
    bug_db_url = guess_bug_database_url_from_repo_url(repo_url)
    if bug_db_url:
        yield UpstreamDatum(
            "Bug-Database",
            bug_db_url,
            certainty=min_certainty(["likely", upstream_metadata["Repository"].certainty]),
        )


def _extrapolate_bug_submit_from_bug_db(upstream_metadata, net_access):
    bug_submit_url = bug_submit_url_from_bug_database_url(
        upstream_metadata["Bug-Database"].value
    )
    if bug_submit_url:
        yield UpstreamDatum(
            "Bug-Submit", bug_submit_url, certainty=upstream_metadata["Bug-Database"].certainty
        )


def _extrapolate_bug_db_from_bug_submit(upstream_metadata, net_access):
    bug_db_url = bug_database_url_from_bug_submit_url(
        upstream_metadata["Bug-Submit"].value
    )
    if bug_db_url:
        yield UpstreamDatum(
            "Bug-Database", bug_db_url, certainty=upstream_metadata["Bug-Submit"].certainty
        )


def _copy_bug_db_field(upstream_metadata, net_access):
    ret = UpstreamDatum(
        "Bug-Database",
        upstream_metadata["Bugs-Database"].value,
        certainty=upstream_metadata["Bugs-Database"].certainty,
        origin=upstream_metadata["Bugs-Database"].origin,
    )
    del upstream_metadata["Bugs-Database"]
    return ret


def _extrapolate_security_contact_from_security_md(upstream_metadata, net_access):
    repository_url = upstream_metadata["Repository"]
    security_md_path = upstream_metadata["Security-MD"]
    security_url = browse_url_from_repo_url(
        repository_url.value, subpath=security_md_path.value
    )
    if security_url is None:
        return
    yield UpstreamDatum(  # noqa: B901
        "Security-Contact",
        security_url,
        certainty=min_certainty([repository_url.certainty, security_md_path.certainty]),
        origin=security_md_path.origin,
    )


def _extrapolate_contact_from_maintainer(upstream_metadata, net_access):
    maintainer = upstream_metadata["Maintainer"]
    yield UpstreamDatum(
        "Contact",
        str(maintainer.value),
        certainty=min_certainty([maintainer.certainty]),
        origin=maintainer.origin,
    )


def _extrapolate_homepage_from_repository_browse(upstream_metadata, net_access):
    browse_url = upstream_metadata["Repository-Browse"].value
    # Some hosting sites are commonly used as Homepage
    # TODO(jelmer): Maybe check that there is a README file that
    # can serve as index?
    forge = find_forge(browse_url)
    if forge and forge.repository_browse_can_be_homepage:
        yield UpstreamDatum("Homepage", browse_url, certainty="possible")


def _consult_homepage(upstream_metadata, net_access):
    if not net_access:
        return
    from .homepage import guess_from_homepage

    for entry in guess_from_homepage(upstream_metadata["Homepage"].value):
        entry.certainty = min_certainty(
            [upstream_metadata["Homepage"].certainty, entry.certainty]
        )
        yield entry


EXTRAPOLATE_FNS = [
    (["Homepage"], ["Repository"], _extrapolate_repository_from_homepage),
    (["Repository-Browse"], ["Homepage"], _extrapolate_homepage_from_repository_browse),
    (["Bugs-Database"], ["Bug-Database"], _copy_bug_db_field),
    (["Bug-Database"], ["Repository"], _extrapolate_repository_from_bug_db),
    (
        ["Repository"],
        ["Repository-Browse"],
        _extrapolate_repository_browse_from_repository,
    ),
    (
        ["Repository-Browse"],
        ["Repository"],
        _extrapolate_repository_from_repository_browse,
    ),
    (["Repository"], ["Bug-Database"], _extrapolate_bug_database_from_repository),
    (["Bug-Database"], ["Bug-Submit"], _extrapolate_bug_submit_from_bug_db),
    (["Bug-Submit"], ["Bug-Database"], _extrapolate_bug_db_from_bug_submit),
    (["Download"], ["Repository"], _extrapolate_repository_from_download),
    (["Repository"], ["Name"], _extrapolate_name_from_repository),
    (
        ["Repository", "Security-MD"],
        "Security-Contact",
        _extrapolate_security_contact_from_security_md,
    ),
    (["Maintainer"], ["Contact"], _extrapolate_contact_from_maintainer),
    (["Homepage"], ["Bug-Database", "Repository"], _consult_homepage),
]


def extend_upstream_metadata(
    upstream_metadata,
    path,
    minimum_certainty=None,
    net_access=False,
    consult_external_directory=False,
):
    """Extend a set of upstream metadata."""
    # TODO(jelmer): Use EXTRAPOLATE_FNS mechanism for this?
    for field in [
        "Homepage",
        "Bug-Database",
        "Bug-Submit",
        "Repository",
        "Repository-Browse",
        "Download",
    ]:
        if field not in upstream_metadata:
            continue
        project = extract_sf_project_name(upstream_metadata[field].value)
        if project:
            certainty = min_certainty(["likely", upstream_metadata[field].certainty])
            upstream_metadata["Archive"] = UpstreamDatum(
                "Archive", "SourceForge", certainty=certainty
            )
            upstream_metadata["SourceForge-Project"] = UpstreamDatum(
                "SourceForge-Project", project, certainty=certainty
            )
            break

    archive = upstream_metadata.get("Archive")
    if (
        archive
        and archive.value == "SourceForge"
        and "SourceForge-Project" in upstream_metadata
        and net_access
    ):
        sf_project = upstream_metadata["SourceForge-Project"].value
        sf_certainty = upstream_metadata["Archive"].certainty
        try:
            SourceForge.extend_metadata(upstream_metadata, sf_project, sf_certainty)
        except NoSuchForgeProject:
            del upstream_metadata["SourceForge-Project"]

    if (
        archive
        and archive.value == "Hackage"
        and "Hackage-Package" in upstream_metadata
        and net_access
    ):
        hackage_package = upstream_metadata["Hackage-Package"].value
        hackage_certainty = upstream_metadata["Archive"].certainty

        try:
            Hackage.extend_metadata(
                upstream_metadata, hackage_package, hackage_certainty
            )
        except NoSuchPackage:
            del upstream_metadata["Hackage-Package"]

    if (
        archive
        and archive.value == "crates.io"
        and "Cargo-Crate" in upstream_metadata
        and net_access
    ):
        crate = upstream_metadata["Cargo-Crate"].value
        crates_io_certainty = upstream_metadata["Archive"].certainty
        try:
            CratesIo.extend_metadata(upstream_metadata, crate, crates_io_certainty)
        except NoSuchPackage:
            del upstream_metadata["Cargo-Crate"]

    if (
        archive
        and archive.value == "Pecl"
        and "Pecl-Package" in upstream_metadata
        and net_access
    ):
        pecl_package = upstream_metadata["Pecl-Package"].value
        pecl_certainty = upstream_metadata["Archive"].certainty
        Pecl.extend_metadata(upstream_metadata, pecl_package, pecl_certainty)

    if net_access and consult_external_directory:
        # TODO(jelmer): Don't assume debian/control exists
        from debian.deb822 import Deb822

        try:
            with open(os.path.join(path, "debian/control")) as f:
                package = Deb822(f)["Source"]
        except FileNotFoundError:
            # Huh, okay.
            pass
        else:
            extend_from_lp(upstream_metadata, minimum_certainty, package)
            Aur.extend_metadata(upstream_metadata, package, minimum_certainty)
            Gobo.extend_metadata(upstream_metadata, package, minimum_certainty)
            extend_from_repology(upstream_metadata, minimum_certainty, package)

    _extrapolate_fields(upstream_metadata, net_access=net_access)


DEFAULT_ITERATION_LIMIT = 100


def _extrapolate_fields(
    upstream_metadata,
    net_access: bool = False,
    iteration_limit: int = DEFAULT_ITERATION_LIMIT,
):
    changed = True
    iterations = 0
    while changed:
        changed = False
        iterations += 1
        if iterations > iteration_limit:
            raise Exception("hit iteration limit %d" % iteration_limit)
        for from_fields, to_fields, fn in EXTRAPOLATE_FNS:
            from_certainties: Optional[List[str]] = []
            from_value = None
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
            assert from_value is not None
            from_certainty = min_certainty(from_certainties)
            old_to_values = {
                to_field: upstream_metadata.get(to_field) for to_field in to_fields
            }
            assert not [old_value for old_value in old_to_values.values() if old_value is not None and old_value.certainty is None], "%r" % old_to_values
            if all(
                [
                    old_value is not None
                    and certainty_to_confidence(from_certainty)  # type: ignore
                    > certainty_to_confidence(old_value.certainty)
                    for old_value in old_to_values.values()
                ]
            ):
                continue
            changes = update_from_guesses(
                upstream_metadata, fn(upstream_metadata, net_access)
            )
            if changes:
                logger.debug(
                    "Extrapolating (%r ⇒ %r) from ('%s: %s', %s)",
                    [
                        f"{us.field}: {us.value}"
                        for us in old_to_values.values()
                        if us
                    ],
                    [f"{us.field}: {us.value}" for us in changes if us],
                    from_value.field,
                    from_value.value,
                    from_value.certainty,
                )
                changed = True


def verify_screenshots(urls: List[str]) -> Iterator[Tuple[str, Optional[bool]]]:
    headers = {"User-Agent": USER_AGENT}
    for url in urls:
        try:
            response = urlopen(
                Request(url, headers=headers, method="HEAD"),
                timeout=DEFAULT_URLLIB_TIMEOUT,
            )
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
    upstream_metadata: UpstreamMetadata, version: Optional[str] = None
):  # noqa: C901
    """Check upstream metadata.

    This will make network connections, etc.
    """
    repository = upstream_metadata.get("Repository")
    if repository:
        try:
            canonical_url = check_repository_url_canonical(
                repository.value, version=version
            )
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug("Deleting invalid Repository URL %s: %s", e.url, e.reason)
            del upstream_metadata["Repository"]
        else:
            repository.value = canonical_url
            if repository.certainty == "confident":
                repository.certainty = "certain"
            derived_browse_url = browse_url_from_repo_url(repository.value)
            browse_repo = upstream_metadata.get("Repository-Browse")
            if browse_repo and derived_browse_url == browse_repo.value:
                browse_repo.certainty = repository.certainty
    homepage = upstream_metadata.get("Homepage")
    if homepage:
        try:
            canonical_url = check_url_canonical(homepage.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug("Deleting invalid Homepage URL %s: %s", e.url, e.reason)
            del upstream_metadata["Homepage"]
        else:
            homepage.value = canonical_url
            if certainty_sufficient(homepage.certainty, "likely"):
                homepage.certainty = "certain"
    repository_browse = upstream_metadata.get("Repository-Browse")
    if repository_browse:
        try:
            canonical_url = check_url_canonical(repository_browse.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug(
                "Deleting invalid Repository-Browse URL %s: %s", e.url, e.reason
            )
            del upstream_metadata["Repository-Browse"]
        else:
            repository_browse.value = canonical_url
            if certainty_sufficient(repository_browse.certainty, "likely"):
                repository_browse.certainty = "certain"
    bug_database = upstream_metadata.get("Bug-Database")
    if bug_database:
        try:
            canonical_url = check_bug_database_canonical(bug_database.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug("Deleting invalid Bug-Database URL %s: %s", e.url, e.reason)
            del upstream_metadata["Bug-Database"]
        else:
            bug_database.value = canonical_url
            if certainty_sufficient(bug_database.certainty, "likely"):
                bug_database.certainty = "certain"
    bug_submit = upstream_metadata.get("Bug-Submit")
    if bug_submit:
        try:
            canonical_url = check_bug_submit_url_canonical(bug_submit.value)
        except UrlUnverifiable:
            pass
        except InvalidUrl as e:
            logger.debug("Deleting invalid Bug-Submit URL %s: %s", e.url, e.reason)
            del upstream_metadata["Bug-Submit"]
        else:
            bug_submit.value = canonical_url
            if certainty_sufficient(bug_submit.certainty, "likely"):
                bug_submit.certainty = "certain"
    screenshots = upstream_metadata.get("Screenshots")
    if screenshots and screenshots.certainty == "likely":
        newvalue = []
        screenshots.certainty = "certain"
        for url, status in verify_screenshots(screenshots.value):
            if status is True:
                newvalue.append(url)
            elif status is False:
                pass
            else:
                screenshots.certainty = "likely"
        screenshots.value = newvalue


def guess_from_pecl_package(package):
    url = "https://pecl.php.net/packages/%s" % package
    headers = {"User-Agent": USER_AGENT}
    try:
        f = urlopen(Request(url, headers=headers), timeout=PECL_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        return
    except (socket.timeout, TimeoutError):
        logger.warning("timeout contacting pecl, ignoring: %s", url)
        return
    try:
        from bs4 import BeautifulSoup, Tag
    except ModuleNotFoundError:
        logger.warning("bs4 missing so unable to scan pecl page, ignoring %s", url)
        return
    bs = BeautifulSoup(f.read(), features="lxml")
    tag = bs.find("a", text="Browse Source")
    if isinstance(tag, Tag):
        yield "Repository-Browse", tag.attrs["href"]
    tag = bs.find("a", text="Package Bugs")
    if isinstance(tag, Tag):
        yield "Bug-Database", tag.attrs["href"]
    label_tag = bs.find("th", text="Homepage")
    if isinstance(label_tag, Tag) and label_tag.parent is not None:
        tag = label_tag.parent.find("a")
        if isinstance(tag, Tag):
            yield "Homepage", tag.attrs["href"]


guess_from_aur = _upstream_ontologist.guess_from_aur
guess_from_launchpad = _upstream_ontologist.guess_from_launchpad
guess_from_gobo = _upstream_ontologist.guess_from_gobo


def fix_upstream_metadata(upstream_metadata: UpstreamMetadata):
    """Fix existing upstream metadata."""
    if "Repository" in upstream_metadata:
        repo = upstream_metadata["Repository"]
        url = repo.value
        url = sanitize_vcs_url(url)
        repo.value = url
    if "Summary" in upstream_metadata:
        summary = upstream_metadata["Summary"]
        summary.value = summary.value.split(". ")[0]
        summary.value = summary.value.rstrip().rstrip(".")
