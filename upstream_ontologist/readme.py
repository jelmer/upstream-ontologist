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

"""README parsing."""

import logging
import platform
import re
from typing import Optional, Tuple, Iterable, List

from . import UpstreamDatum


logger = logging.getLogger(__name__)


def _skip_paragraph(para, metadata):
    if re.match(r'See .* for more (details|information)\.', para.get_text()):
        return True
    if re.match(r'Please refer .*\.', para.get_text()):
        return True
    m = re.match(r'License: (.*)', para.get_text())
    if m:
        metadata.append(UpstreamDatum('License', m.group(1), 'likely'))
        return True
    m = re.match('More documentation .* at http.*', para.get_text())
    if m:
        return True
    m = re.match('See http.*', para.get_text())
    if m:
        return True
    m = re.match(
            r'This software is freely distributable under the (.*) license.*',
            para.get_text())
    if m:
        metadata.append(UpstreamDatum('License', m.group(1), 'likely'))
        return True
    for c in para.children:
        if isinstance(c, str) and not c.strip():
            continue
        if c.name == 'a':
            if len(list(c.children)) != 1:
                name = None
            elif isinstance(list(c.children)[0], str):
                name = list(c.children)[0]
            elif list(c.children)[0].name == 'img':
                name = list(c.children)[0].get('alt')
            else:
                name = None
            if name == 'CRAN':
                metadata.append(UpstreamDatum('Archive', 'CRAN', 'confident'))
            elif name:
                m = re.match('(.*) License', name)
                if m:
                    metadata.append(UpstreamDatum('X-License', m.group(1), 'likely'))
            continue
        break
    else:
        return True
    return False


def _description_from_basic_soup(soup) -> Tuple[Optional[str], Iterable[UpstreamDatum]]:
    # Drop any headers
    metadata = []
    if soup is None:
        return None, {}
    # First, skip past the first header.
    for el in soup.children:
        if el.name in ('h1', 'h2', 'h3'):
            summary = None
            name = None
            if ':' in el.text:
                name, summary = el.text.split(':', 1)
            elif ' - ' in el.text:
                name, summary = el.text.split(' - ', 1)
            elif ' -- ' in el.text:
                name, summary = el.text.split(' -- ', 1)
            elif el.text:
                name = el.text
            if name:
                metadata.append(UpstreamDatum('Name', name, 'likely'))
            if summary:
                metadata.append(UpstreamDatum('Name', summary, 'likely'))
            el.decompose()
            break
        elif isinstance(el, str):
            pass
        else:
            break

    paragraphs: List[str] = []
    for el in soup.children:
        if isinstance(el, str):
            continue
        if el.name == 'p':
            if _skip_paragraph(el, metadata):
                if len(paragraphs) > 0:
                    break
                else:
                    continue
            while [c.name for c in el.children if not isinstance(c, str)] in (['pre'], ['code']):
                el = list(el.children)[0]
            if el.get_text().strip():
                paragraphs.append(el.get_text() + '\n')
        elif el.name == 'ul':
            paragraphs.append(
                ''.join(
                    '* %s\n' % li.get_text()
                    for li in el.findAll('li')))
        elif re.match('h[0-9]', el.name):
            if len(paragraphs) == 0 and el.get_text() in ('About', ):
                continue
            break

    if len(paragraphs) >= 1 and len(paragraphs) < 6:
        return '\n'.join(paragraphs), metadata
    return None, metadata


def description_from_readme_md(md_text: str) -> Tuple[Optional[str], Iterable[UpstreamDatum]]:
    """Description from README.md."""
    try:
        import markdown
    except ModuleNotFoundError:
        logger.debug('markdown not available, not parsing README.md')
        return None, {}
    html_text = markdown.markdown(md_text)
    try:
        from bs4 import BeautifulSoup, FeatureNotFound
    except ModuleNotFoundError:
        logger.debug('BeautifulSoup not available, not parsing README.md')
        return None, {}
    try:
        soup = BeautifulSoup(html_text, 'lxml')
    except FeatureNotFound:
        logger.debug('lxml not available, not parsing README.md')
        return None, {}
    return _description_from_basic_soup(soup.body)


def description_from_readme_rst(rst_text: str) -> Tuple[Optional[str], Iterable[UpstreamDatum]]:
    """Description from README.rst."""
    if platform.python_implementation() == "PyPy":
        logger.debug('docutils does not appear to work on PyPy, skipping README.rst.')
        return None, {}
    try:
        from docutils.core import publish_parts
    except ModuleNotFoundError:
        logger.debug('docutils not available, not parsing README.rst')
        return None, {}

    from docutils.writers.html4css1 import Writer
    settings = {'initial_header_level': 2, 'report_level': 0}
    html_text = publish_parts(
        rst_text, writer=Writer(), settings_overrides=settings).get('html_body')
    try:
        from bs4 import BeautifulSoup, FeatureNotFound
    except ModuleNotFoundError:
        logger.debug('BeautifulSoup not available, not parsing README.rst')
        return None, {}
    try:
        soup = BeautifulSoup(html_text, 'lxml')
    except FeatureNotFound:
        logger.debug('lxml not available, not parsing README.rst')
        return None, {}
    return _description_from_basic_soup(list(soup.body.children)[0])
