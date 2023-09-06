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

__all__ = [
    "plausible_url",
    "plausible_browse_url",
    "sanitize_url",
    "is_gitlab_site",
    "browse_url_from_repo_url",
    "probe_gitlab_host",
    "guess_repo_from_url",
    "probe_upstream_branch_url",
    "check_repository_url_canonical",
    "unsplit_vcs_url",
    "browse_url_from_repo_url",
    "find_public_repo_url",
]

import logging
from typing import Optional, Union, List, Tuple, Callable

from urllib.parse import urlparse, urlunparse, ParseResult

from ._upstream_ontologist import (  # noqa: F401
    drop_vcs_in_scheme,
    canonical_git_repo_url,
    unsplit_vcs_url,
    plausible_vcs_browse_url as plausible_browse_url,
    plausible_vcs_url as plausible_url,
    probe_upstream_branch_url,
    check_repository_url_canonical,
    guess_repo_from_url,
    is_gitlab_site,
    probe_gitlab_host,
    browse_url_from_repo_url,
    find_public_repo_url,
)


KNOWN_GITLAB_SITES = [
    "salsa.debian.org",
    "invent.kde.org",
    "0xacab.org",
]


logger = logging.getLogger(__name__)


SECURE_SCHEMES = ["https", "git+ssh", "bzr+ssh", "hg+ssh", "ssh", "svn+ssh"]


def try_open_branch(url: str, branch_name: Optional[str] = None):
    import breezy.ui
    from breezy.controldir import ControlDir

    old_ui = breezy.ui.ui_factory
    breezy.ui.ui_factory = breezy.ui.SilentUIFactory()
    try:
        c = ControlDir.open(url)
        b = c.open_branch(name=branch_name)
        b.last_revision()
        return b
    except Exception:
        # TODO(jelmer): Catch more specific exceptions?
        return None
    finally:
        breezy.ui.ui_factory = old_ui


def find_secure_repo_url(
    url: str, branch: Optional[str] = None, net_access: bool = True
) -> Optional[str]:
    parsed_repo_url = urlparse(url)
    if parsed_repo_url.scheme in SECURE_SCHEMES:
        return url

    # Sites we know to be available over https
    if (parsed_repo_url.hostname
        and (
            is_gitlab_site(parsed_repo_url.hostname, net_access)
            or parsed_repo_url.hostname in [
                "github.com",
                "git.launchpad.net",
                "bazaar.launchpad.net",
                "code.launchpad.net",
            ])):
        parsed_repo_url = parsed_repo_url._replace(scheme="https")

    if parsed_repo_url.scheme == "lp":
        parsed_repo_url = parsed_repo_url._replace(
            scheme="https", netloc="code.launchpad.net"
        )

    if parsed_repo_url.hostname in ("git.savannah.gnu.org", "git.sv.gnu.org"):
        if parsed_repo_url.scheme == "http":
            parsed_repo_url = parsed_repo_url._replace(scheme="https")
        else:
            parsed_repo_url = parsed_repo_url._replace(
                scheme="https", path="/git" + parsed_repo_url.path
            )

    if net_access:
        secure_repo_url = parsed_repo_url._replace(scheme="https")
        insecure_branch = try_open_branch(url, branch)
        secure_branch = try_open_branch(urlunparse(secure_repo_url), branch)
        if secure_branch:
            if (
                not insecure_branch
                or secure_branch.last_revision() == insecure_branch.last_revision()
            ):
                parsed_repo_url = secure_repo_url

    if parsed_repo_url.scheme in SECURE_SCHEMES:
        return urlunparse(parsed_repo_url)

    # Can't find a secure URI :(
    return None


def fixup_rcp_style_git_repo_url(url: str) -> str:
    try:
        from breezy.location import rcp_location_to_url
    except ModuleNotFoundError:
        return url

    try:
        repo_url = rcp_location_to_url(url)
    except ValueError:
        return url
    return repo_url


def fix_path_in_port(
    parsed: ParseResult, branch: Optional[str], subpath: Optional[str]
):
    if ":" not in parsed.netloc or parsed.netloc.endswith("]"):
        return None, None, None
    host, port = parsed.netloc.rsplit(":", 1)
    if host.split("@")[-1] not in (KNOWN_GITLAB_SITES + ["github.com"]):
        return None, None, None
    if not port or port.isdigit():
        return None, None, None
    return (
        parsed._replace(path="{}/{}".format(port, parsed.path.lstrip("/")), netloc=host),
        branch,
        subpath,
    )


def fix_gitlab_scheme(parsed, branch, subpath: Optional[str]):
    if is_gitlab_site(parsed.hostname):
        return parsed._replace(scheme="https"), branch, subpath
    return None, None, None


def fix_github_scheme(parsed, branch, subpath: Optional[str]):
    # GitHub no longer supports the git:// scheme
    if parsed.hostname == 'github.com' and parsed.scheme == 'git':
        return parsed._replace(scheme='https'), branch, subpath
    return None, None, None


def fix_salsa_cgit_url(parsed, branch, subpath):
    if parsed.hostname == "salsa.debian.org" and parsed.path.startswith("/cgit/"):
        return parsed._replace(path=parsed.path[5:]), branch, subpath
    return None, None, None


def fix_gitlab_tree_in_url(parsed, branch, subpath):
    if is_gitlab_site(parsed.hostname):
        parts = parsed.path.split("/")
        if len(parts) >= 5 and parts[3] == "tree":
            branch = "/".join(parts[4:])
            return parsed._replace(path="/".join(parts[:3])), branch, subpath
    return None, None, None


def fix_double_slash(parsed, branch, subpath):
    if parsed.path.startswith("//"):
        return parsed._replace(path=parsed.path[1:]), branch, subpath
    return None, None, None


def fix_extra_colon(parsed, branch, subpath):
    return parsed._replace(netloc=parsed.netloc.rstrip(":")), branch, subpath


def drop_git_username(parsed, branch, subpath):
    if parsed.hostname not in ("salsa.debian.org", "github.com"):
        return None, None, None
    if parsed.scheme not in ("git", "http", "https"):
        return None, None, None
    if parsed.username == "git" and parsed.netloc.startswith("git@"):
        return parsed._replace(netloc=parsed.netloc[4:]), branch, subpath
    return None, None, None


def fix_branch_argument(parsed, branch, subpath):
    if parsed.hostname == "github.com":
        # TODO(jelmer): Handle gitlab sites too?
        path_elements = parsed.path.strip("/").split("/")
        if len(path_elements) > 2 and path_elements[2] == "tree":
            return (
                parsed._replace(path="/".join(path_elements[:2])),
                "/".join(path_elements[3:]),
                subpath,
            )
    return None, None, None


def fix_git_gnome_org_url(parsed, branch, subpath):
    if parsed.netloc == "git.gnome.org":
        if parsed.path.startswith("/browse"):
            path = parsed.path[7:]
        else:
            path = parsed.path
        parsed = parsed._replace(
            netloc="gitlab.gnome.org", scheme="https", path="/GNOME" + path
        )
        return parsed, branch, subpath
    return None, None, None


def fix_anongit_url(parsed, branch, subpath):
    if parsed.netloc == "anongit.kde.org" and parsed.scheme == "git":
        parsed = parsed._replace(scheme="https")
        return parsed, branch, subpath
    return None, None, None


def fix_freedesktop_org_url(
    parsed: ParseResult, branch: Optional[str], subpath: Optional[str]
):
    if parsed.netloc == "anongit.freedesktop.org":
        path = parsed.path
        if path.startswith("/git/"):
            path = path[len("/git") :]
        parsed = parsed._replace(
            netloc="gitlab.freedesktop.org", scheme="https", path=path
        )
        return parsed, branch, subpath
    return None, None, None


FIXERS = [
    fix_path_in_port,
    fix_gitlab_scheme,
    fix_github_scheme,
    fix_salsa_cgit_url,
    fix_gitlab_tree_in_url,
    fix_double_slash,
    fix_extra_colon,
    drop_git_username,
    fix_branch_argument,
    fix_git_gnome_org_url,
    fix_anongit_url,
    fix_freedesktop_org_url,
]


def fixup_broken_git_details(
    repo_url: str, branch: Optional[str], subpath: Optional[str]
) -> Tuple[str, Optional[str], Optional[str]]:
    """Attempt to fix up broken Git URLs.

    A common misspelling is to add an extra ":" after the hostname
    """
    parsed = urlparse(repo_url)
    changed = False
    for fn in FIXERS:
        newparsed, newbranch, newsubpath = fn(parsed, branch, subpath)
        if newparsed:
            changed = True
            parsed = newparsed
            branch = newbranch
            subpath = newsubpath

    if changed:
        return urlunparse(parsed), branch, subpath

    return repo_url, branch, subpath


def convert_cvs_list_to_str(urls):
    if not isinstance(urls, list):
        return urls
    if urls[0].startswith(":extssh:") or urls[0].startswith(":pserver:"):
        from breezy.location import cvs_to_url
        return cvs_to_url(urls[0]) + "#" + urls[1]
    return urls


SANITIZERS: List[Callable[[str], str]] = [
    convert_cvs_list_to_str,
    drop_vcs_in_scheme,
    lambda url: fixup_broken_git_details(url, None, None)[0],
    fixup_rcp_style_git_repo_url,
    lambda url: find_public_repo_url(url) or url,
    canonical_git_repo_url,
    lambda url: find_secure_repo_url(url, net_access=False) or url,
]


def sanitize_url(url: Union[str, List[str]]) -> str:
    if isinstance(url, str):
        url = url.strip()
    for sanitizer in SANITIZERS:
        url = sanitizer(url)  # type: ignore
    return url  # type: ignore
