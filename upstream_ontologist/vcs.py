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

from typing import Optional

import socket
import urllib
from urllib.parse import urlparse, urlunparse
from urllib.request import urlopen, Request

from lintian_brush.vcs import (
    sanitize_url,
    )


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
