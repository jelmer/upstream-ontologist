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
import re
from typing import Optional, Tuple, Dict, List


logger = logging.getLogger(__name__)


def _skip_paragraph(para):
    if para.startswith('License: '):
        return True
    if re.match(r'See .* for more (details|information)\.', para):
        return True
    if re.match(r'Please refer .*\.', para):
        return True
    return False


def _description_from_basic_soup(soup) -> Tuple[Optional[str], Dict[str, str]]:
    # Drop any headers
    metadata = {}
    # First, skip past the first header.
    for el in soup.children:
        if el.name == 'h1':
            metadata['Name'] = el.text
            el.decompose()
        elif isinstance(el, str):
            pass
        else:
            break

    paragraphs: List[str] = []
    for el in soup.children:
        if isinstance(el, str):
            continue
        if el.name == 'p':
            if _skip_paragraph(el.get_text()):
                if len(paragraphs) > 0:
                    break
                else:
                    continue
            if el.get_text().strip():
                paragraphs.append(el.get_text() + '\n')
        elif el.name == 'ul':
            paragraphs.append(
                ''.join(
                    '* %s\n' % li.get_text()
                    for li in el.findAll('li')))
        elif re.match('h[0-9]', el.name):
            break
    if len(paragraphs) >= 1 and len(paragraphs) < 6:
        return '\n'.join(paragraphs), metadata
    return None, metadata


def description_from_readme_md(md_text: str) -> Tuple[Optional[str], Dict[str, str]]:
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


def description_from_readme_rst(rst_text: str) -> Tuple[Optional[str], Dict[str, str]]:
    """Description from README.rst."""
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
