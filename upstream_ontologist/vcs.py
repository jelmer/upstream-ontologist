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

import socket
import urllib.error
from urllib.request import urlopen, Request

from . import (
    USER_AGENT,
    DEFAULT_URLLIB_TIMEOUT,
    )


KNOWN_GITLAB_SITES = [
    'gitlab.com',
    'salsa.debian.org',
    'gitlab.gnome.org',
    'gitlab.freedesktop.org',
    'gitlab.labs.nic.cz',
    'invent.kde.org',
    ]


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
