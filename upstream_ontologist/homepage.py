#!/usr/bin/python3
# Copyright (C) 2021 Jelmer Vernooij <jelmer@debian.org>
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

import urllib.error
from urllib.request import Request, urlopen

import logging

from . import UpstreamDatum, USER_AGENT

logger = logging.getLogger(__name__)


def guess_from_homepage(url: str):
    req = Request(url, headers={'User-Agent': USER_AGENT})
    try:
        f = urlopen(req)
    except urllib.error.HTTPError as e:
        logger.warning(
            'unable to access homepage %r: %s', url, e)
        return
    except urllib.error.URLError as e:
        logger.warning(
            'unable to access homepage %r: %s', url, e)
        return
    except ConnectionResetError as e:
        logging.warning(
            'unable to access homepage %r: %s', url, e)
        return
    for entry in _guess_from_page(f.read()):
        entry.origin = url
        yield entry


def _guess_from_page(text: bytes):
    try:
        from bs4 import BeautifulSoup, FeatureNotFound
    except ModuleNotFoundError:
        logger.debug('BeautifulSoup not available, not parsing homepage')
        return
    try:
        soup = BeautifulSoup(text, 'lxml')
    except FeatureNotFound:
        logger.debug('lxml not available, not parsing README.md')
        return
    return _guess_from_soup(soup)


def _guess_from_soup(soup):
    for a in soup.findAll('a'):
        href = a.get('href')
        if a.get('aria-label') in ('github', 'git', 'repository'):
            yield UpstreamDatum('Repository', href, certainty='confident')
