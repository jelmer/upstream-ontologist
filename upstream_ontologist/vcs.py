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
    'plausible_url',
    'plausible_browse_url',
    'sanitize_url',
    'is_gitlab_site',
    'browse_url_from_repo_url',
    ]

from typing import Optional, Union, List

import socket
import urllib
from urllib.parse import urlparse, urlunparse, ParseResult
from urllib.request import urlopen, Request


from . import (
    DEFAULT_URLLIB_TIMEOUT,
    USER_AGENT,
    )


KNOWN_GITLAB_SITES = [
    'salsa.debian.org',
    'invent.kde.org',
    ]


def plausible_browse_url(url: str) -> bool:
    return url.startswith('https://') or url.startswith('http://')


def plausible_url(url: str) -> bool:
    return ':' in url


def unsplit_vcs_url(repo_url: str,
                    branch: Optional[str] = None,
                    subpath: Optional[str] = None) -> str:
    """Unsplit a Debian VCS URL.

    Args:
      repo_url: Repository URL
      branch: Branch name
      subpath: Subpath in the tree
    Returns: full URL
    """
    url = repo_url
    if branch:
        url = '%s -b %s' % (url, branch)
    if subpath:
        url = '%s [%s]' % (url, subpath)
    return url


def probe_gitlab_host(hostname: str):
    headers = {'User-Agent': USER_AGENT, 'Accept': 'application/json'}
    try:
        urlopen(
            Request('https://%s/api/v4/version' % hostname, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.status == 401:
            import json
            if json.loads(e.read()) == {"message": "401 Unauthorized"}:
                return True
    except (socket.timeout, urllib.error.URLError):
        # Probably not?
        return False
    return False


def is_gitlab_site(hostname: str, net_access: bool = False) -> bool:
    if hostname is None:
        return False
    if hostname in KNOWN_GITLAB_SITES:
        return True
    if hostname.startswith('gitlab.'):
        return True
    if net_access:
        return probe_gitlab_host(hostname)
    return False


def browse_url_from_repo_url(
        url: str, subpath: Optional[str] = None) -> Optional[str]:
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path = '/'.join(parsed_url.path.split('/')[:3])
        if path.endswith('.git'):
            path = path[:-4]
        if subpath is not None:
            path += '/tree/HEAD/' + subpath
        return urlunparse(
            ('https', 'github.com', path,
             None, None, None))
    if parsed_url.netloc in ('code.launchpad.net', 'launchpad.net'):
        if subpath is not None:
            path = parsed_url.path + '/view/head:/' + subpath
            return urlunparse(
                ('https', 'bazaar.launchpad.net', path,
                 parsed_url.query, parsed_url.params, parsed_url.fragment))
        else:
            return urlunparse(
                ('https', 'code.launchpad.net', parsed_url.path,
                 parsed_url.query, parsed_url.params, parsed_url.fragment))
    if is_gitlab_site(parsed_url.netloc):
        path = parsed_url.path
        if path.endswith('.git'):
            path = path[:-4]
        if subpath is not None:
            path += '/-/blob/HEAD/' + subpath
        return urlunparse(('https', parsed_url.netloc, path, None, None, None))
    if parsed_url.netloc == 'svn.apache.org':
        path_elements = parsed_url.path.strip('/').split('/')
        if path_elements[:2] != ['repos', 'asf']:
            return None
        path_elements.pop(0)
        path_elements[0] = 'viewvc'
        if subpath is not None:
            path_elements.append(subpath)
        return urlunparse(
            ('https', parsed_url.netloc, '/'.join(path_elements), None, None,
             None))
    if parsed_url.hostname in ('git.savannah.gnu.org', 'git.sv.gnu.org'):
        path_elements = parsed_url.path.strip('/').split('/')
        if parsed_url.scheme == 'https' and path_elements[0] == 'git':
            path_elements.pop(0)
        # Why cgit and not gitweb?
        path_elements.insert(0, 'cgit')
        if subpath is not None:
            path_elements.append('tree')
            path_elements.append(subpath)
        return urlunparse(
            ('https', parsed_url.netloc, '/'.join(path_elements), None,
             None, None))

    return None


SECURE_SCHEMES = ['https', 'git+ssh', 'bzr+ssh', 'hg+ssh', 'ssh', 'svn+ssh']


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
        url: str, branch: Optional[str] = None,
        net_access: bool = True) -> Optional[str]:
    parsed_repo_url = urlparse(url)
    if parsed_repo_url.scheme in SECURE_SCHEMES:
        return url

    # Sites we know to be available over https
    if (parsed_repo_url.hostname and (
            is_gitlab_site(parsed_repo_url.hostname, net_access) or
            parsed_repo_url.hostname in [
                'github.com', 'git.launchpad.net', 'bazaar.launchpad.net',
                'code.launchpad.net'])):
        parsed_repo_url = parsed_repo_url._replace(scheme='https')

    if parsed_repo_url.scheme == 'lp':
        parsed_repo_url = parsed_repo_url._replace(
            scheme='https', netloc='code.launchpad.net')

    if parsed_repo_url.hostname in ('git.savannah.gnu.org', 'git.sv.gnu.org'):
        if parsed_repo_url.scheme == 'http':
            parsed_repo_url = parsed_repo_url._replace(scheme='https')
        else:
            parsed_repo_url = parsed_repo_url._replace(
                scheme='https', path='/git' + parsed_repo_url.path)

    if net_access:
        secure_repo_url = parsed_repo_url._replace(scheme='https')
        insecure_branch = try_open_branch(url, branch)
        secure_branch = try_open_branch(urlunparse(secure_repo_url), branch)
        if secure_branch:
            if (not insecure_branch or
                    secure_branch.last_revision() ==
                    insecure_branch.last_revision()):
                parsed_repo_url = secure_repo_url

    if parsed_repo_url.scheme in SECURE_SCHEMES:
        return urlunparse(parsed_repo_url)

    # Can't find a secure URI :(
    return None


def canonical_git_repo_url(repo_url: str) -> str:
    parsed_url = urlparse(repo_url)
    if (is_gitlab_site(parsed_url.netloc) or
            parsed_url.netloc in ['github.com']):
        if not parsed_url.path.rstrip('/').endswith('.git'):
            parsed_url = parsed_url._replace(
                path=parsed_url.path.rstrip('/') + '.git')
        return urlunparse(parsed_url)
    return repo_url


def find_public_repo_url(repo_url: str) -> Optional[str]:
    parsed = urlparse(repo_url)
    revised_url = None
    if parsed.hostname == 'github.com':
        if parsed.scheme in ('https', 'http', 'git'):
            return repo_url
        revised_url = urlunparse(
                ('https', 'github.com', parsed.path, None, None, None))
    if parsed.hostname and is_gitlab_site(parsed.hostname):
        # Not sure if gitlab even support plain http?
        if parsed.scheme in ('https', 'http'):
            return repo_url
        if parsed.scheme == 'ssh':
            revised_url = urlunparse(
                ('https', parsed.hostname, parsed.path, None, None, None))
    if parsed.hostname in (
            'code.launchpad.net', 'bazaar.launchpad.net', 'git.launchpad.net'):
        if parsed.scheme.startswith('http') or parsed.scheme == 'lp':
            return repo_url
        if parsed.scheme in ('ssh', 'bzr+ssh'):
            revised_url = urlunparse(
                ('https', parsed.hostname, parsed.path, None, None, None))

    if revised_url:
        return revised_url

    return None


def fixup_rcp_style_git_repo_url(url: str) -> str:
    from breezy.location import rcp_location_to_url
    try:
        repo_url = rcp_location_to_url(url)
    except ValueError:
        return url
    return repo_url


def drop_vcs_in_scheme(url: str) -> str:
    if url.startswith('git+http:') or url.startswith('git+https:'):
        url = url[4:]
    if url.startswith('hg+https:') or url.startswith('hg+http'):
        url = url[3:]
    if url.startswith('bzr+lp:') or url.startswith('bzr+http'):
        url = url.split('+', 1)[1]
    return url


def fix_path_in_port(
        parsed: ParseResult, branch: Optional[str], subpath: Optional[str]):
    if ':' not in parsed.netloc or parsed.netloc.endswith(']'):
        return None, None, None
    host, port = parsed.netloc.rsplit(':', 1)
    if host.split('@')[-1] not in (KNOWN_GITLAB_SITES + ['github.com']):
        return None, None, None
    if not port or port.isdigit():
        return None, None, None
    return parsed._replace(
        path='%s/%s' % (port, parsed.path.lstrip('/')),
        netloc=host), branch, subpath


def fix_gitlab_scheme(parsed, branch, subpath: Optional[str]):
    if is_gitlab_site(parsed.hostname):
        return parsed._replace(scheme='https'), branch, subpath
    return None, None, None


def fix_salsa_cgit_url(parsed, branch, subpath):
    if (parsed.hostname == 'salsa.debian.org' and
            parsed.path.startswith('/cgit/')):
        return parsed._replace(path=parsed.path[5:]), branch, subpath
    return None, None, None


def fix_gitlab_tree_in_url(parsed, branch, subpath):
    if is_gitlab_site(parsed.hostname):
        parts = parsed.path.split('/')
        if len(parts) >= 5 and parts[3] == 'tree':
            branch = '/'.join(parts[4:])
            return parsed._replace(path='/'.join(parts[:3])), branch, subpath
    return None, None, None


def fix_double_slash(parsed, branch, subpath):
    if parsed.path.startswith('//'):
        return parsed._replace(path=parsed.path[1:]), branch, subpath
    return None, None, None


def fix_extra_colon(parsed, branch, subpath):
    return parsed._replace(netloc=parsed.netloc.rstrip(':')), branch, subpath


def drop_git_username(parsed, branch, subpath):
    if parsed.hostname not in ('salsa.debian.org', 'github.com'):
        return None, None, None
    if parsed.scheme not in ('git', 'http', 'https'):
        return None, None, None
    if parsed.username == 'git' and parsed.netloc.startswith('git@'):
        return parsed._replace(netloc=parsed.netloc[4:]), branch, subpath
    return None, None, None


def fix_branch_argument(parsed, branch, subpath):
    if parsed.hostname != 'github.com':
        return None, None, None
    # TODO(jelmer): Handle gitlab sites too?
    path_elements = parsed.path.strip('/').split('/')
    if len(path_elements) > 2 and path_elements[2] == 'tree':
        return (parsed._replace(path='/'.join(path_elements[:2])),
                '/'.join(path_elements[3:]), subpath)
    return None, None, None


def fix_git_gnome_org_url(parsed, branch, subpath):
    if parsed.netloc == 'git.gnome.org':
        if parsed.path.startswith('/browse'):
            path = parsed.path[7:]
        else:
            path = parsed.path
        parsed = parsed._replace(
            netloc='gitlab.gnome.org', scheme='https',
            path='/GNOME' + path)
        return parsed, branch, subpath
    return None, None, None


def fix_anongit_url(parsed, branch, subpath):
    if parsed.netloc == 'anongit.kde.org' and parsed.scheme == 'git':
        parsed = parsed._replace(scheme='https')
        return parsed, branch, subpath
    return None, None, None


def fix_freedesktop_org_url(
        parsed: ParseResult, branch: Optional[str], subpath: Optional[str]):
    if parsed.netloc == 'anongit.freedesktop.org':
        path = parsed.path
        if path.startswith('/git/'):
            path = path[len('/git'):]
        parsed = parsed._replace(
            netloc='gitlab.freedesktop.org', scheme='https',
            path=path)
        return parsed, branch, subpath
    return None, None, None


FIXERS = [
    fix_path_in_port,
    fix_gitlab_scheme,
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
        repo_url: str, branch: Optional[str], subpath: Optional[str]) -> str:
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
    if urls[0].startswith(':extssh:') or urls[0].startswith(':pserver:'):
        try:
            from breezy.location import cvs_to_url
        except ImportError:
            from breezy.location import pserver_to_url as cvs_to_url
            if urls[0].startswith(':extssh:'):
                raise NotImplementedError(
                    'unable to deal with extssh CVS locations.')
        return cvs_to_url(urls[0]) + '#' + urls[1]
    return urls


SANITIZERS = [
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
        url = sanitizer(url)
    return url
