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
from urllib.parse import urlparse

from . import UpstreamDatum


logger = logging.getLogger(__name__)


def _skip_paragraph(para, metadata):  # noqa: C901
    if re.match(r'See .* for more (details|information)\.', para):
        return True
    if re.match(r'See .* for instructions', para):
        return True
    if re.match(r'Please refer .*\.', para):
        return True
    m = re.match(r'It is licensed under (.*)', para)
    if m:
        metadata.append(UpstreamDatum('X-License', m.group(1), 'possible'))
        return True
    m = re.match(r'License: (.*)', para, re.I)
    if m:
        metadata.append(UpstreamDatum('X-License', m.group(1), 'likely'))
        return True
    m = re.match('(Home page|homepage_url|Main website|Website|Homepage): (.*)', para, re.I)
    if m:
        url = m.group(2)
        if url.startswith('<') and url.endswith('>'):
            url = url[1:-1]
        metadata.append(UpstreamDatum('Homepage', url, 'likely'))
        return True
    m = re.match('More documentation .* at http.*', para)
    if m:
        return True
    m = re.match('Documentation (can be found|is hosted|is available) (at|on) ([^ ]+)', para)
    if m:
        metadata.append(UpstreamDatum('Documentation', m.group(3), 'likely'))
        return True
    m = re.match(r'Documentation for (.*)\s+(can\s+be\s+found|is\s+hosted)\s+(at|on)\s+([^ ]+)', para)
    if m:
        metadata.append(UpstreamDatum('Name', m.group(1), 'possible'))
        metadata.append(UpstreamDatum('Documentation', m.group(4), 'likely'))
        return True
    if re.match(r'Documentation[, ].*found.*(at|on).*\.', para, re.S):
        return True
    m = re.match('See (http.*|gopkg.in.*|github.com.*)', para)
    if m:
        return True
    m = re.match('Available on (.*)', para)
    if m:
        return True
    m = re.match(
        r'This software is freely distributable under the (.*) license.*',
        para)
    if m:
        metadata.append(UpstreamDatum('X-License', m.group(1), 'likely'))
        return True
    m = re.match(r'This .* is hosted at .*', para)
    if m:
        return True
    m = re.match(r'This code has been developed by .*', para)
    if m:
        return True
    if para.startswith('Download and install using:'):
        return True
    m = re.match('Bugs should be reported by .*', para)
    if m:
        return True
    m = re.match(r'The bug tracker can be found at (http[^ ]+[^.])', para)
    if m:
        metadata.append(UpstreamDatum('Bug-Database', m.group(1), 'likely'))
        return True
    m = re.match(r'Copyright (\(c\) |)(.*)', para)
    if m:
        metadata.append(UpstreamDatum('X-Copyright', m.group(2), 'possible'))
        return True
    if re.match('You install .*', para):
        return True
    if re.match('This .* is free software; .*', para):
        return True
    m = re.match('Please report any bugs(.*) to <(.*)>', para)
    if m:
        metadata.append(UpstreamDatum('Bugs-Submit', m.group(2), 'possible'))
        return True
    if re.match('Share and Enjoy', para, re.I):
        return True
    lines = para.splitlines(False)
    if lines and lines[0].strip() in ('perl Makefile.PL', 'make', './configure'):
        return True
    if re.match('For further information, .*', para):
        return True
    if re.match('Further information .*', para):
        return True
    m = re.match(r'A detailed Changelog can be found.*:\s+(http.*)', para, re.I)
    if m:
        metadata.append(UpstreamDatum('Changelog', m.group(1), 'possible'))
        return True


def _skip_paragraph_block(para, metadata):  # noqa: C901
    if _skip_paragraph(para.get_text(), metadata):
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
            if name in ('CRAN', 'CRAN_Status_Badge', 'CRAN_Logs_Badge'):
                metadata.append(UpstreamDatum('Archive', 'CRAN', 'confident'))
            elif name == 'Gitter':
                parsed_url = urlparse(c.get('href'))
                metadata.append(UpstreamDatum(
                    'Repository',
                    'https://github.com/%s' % '/'.join(parsed_url.path.strip('/').split('/')[:2]),
                    'confident'))
            elif name and name.lower() == 'build status':
                parsed_url = urlparse(c.get('href'))
                if parsed_url.hostname == 'travis-ci.org':
                    metadata.append(UpstreamDatum(
                        'Repository',
                        'https://github.com/%s' % '/'.join(parsed_url.path.strip('/').split('/')[:2]),
                        'confident'))
            elif name and name.lower() == 'documentation':
                metadata.append(UpstreamDatum(
                    'Documentation', c.get('href'), 'confident'))
            elif name and name.lower() == 'api docs':
                metadata.append(UpstreamDatum(
                    'X-API-Documentation', c.get('href'), 'confident'))
            elif name and name.lower() == 'downloads':
                metadata.append(UpstreamDatum(
                    'X-Download', c.get('href'), 'confident'))
            elif name and name.lower() == 'crates.io':
                href = c.get('href')
                if href.startswith('https://crates.io/crates/'):
                    metadata.append(UpstreamDatum(
                        'X-Cargo-Crate', href.rsplit('/')[-1], 'confident'))
            elif name:
                m = re.match('(.*) License', name)
                if m:
                    metadata.append(UpstreamDatum('X-License', m.group(1), 'likely'))
                else:
                    logging.debug('Unhandled field %r in README', name)
            continue
        break
    else:
        return True
    if para.get_text() == '':
        return True
    return False


def render(el):
    return el.get_text()


def _parse_first_header_text(text):
    m = re.fullmatch('([A-Za-z]+) ([0-9.]+)', text)
    if m:
        return m.group(1), None, m.group(2)
    m = re.fullmatch('([A-Za-z]+): (.+)', text)
    if m:
        return m.group(1), m.group(2), None
    m = re.fullmatch('([A-Za-z]+) - (.+)', text)
    if m:
        return m.group(1), m.group(2), None
    m = re.fullmatch('([A-Za-z]+) -- (.+)', text)
    if m:
        return m.group(1), m.group(2), None
    m = re.fullmatch('([A-Za-z]+) version ([^ ]+)', text)
    if m:
        name, version = text.split(' version ', 1)
        summary = None
        return name, summary, version
    return None, None, None


def _parse_first_header(el):
    name, summary, version = _parse_first_header_text(el.get_text())
    if not name and el.get_text():
        name = el.get_text()
    if name:
        if 'installation' in name.lower():
            certainty = 'possible'
        else:
            certainty = 'likely'
        if name.startswith('About '):
            name = name[len('About '):]
        yield UpstreamDatum('Name', name.strip(), certainty)
    if summary:
        yield UpstreamDatum('X-Summary', summary, 'likely')
    if version:
        yield UpstreamDatum('X-Version', version, 'likely')


def _is_semi_header(el):
    if el.name != 'p':
        return False
    if el.get_text().strip() == 'INSTALLATION':
        return True
    if el.get_text().count('\n') > 0:
        return False
    m = re.match(r'([a-z-A-Z0-9]+) - ([^\.]+)', el.get_text())
    if m:
        return True
    return False


def _ul_is_field_list(el):
    names = ['Issues', 'Home', 'Documentation', 'License']
    for li in el.findAll('li'):
        m = re.match(r'([A-Za-z]+)\s*:.*', li.get_text().strip())
        if not m or m.group(1) not in names:
            return False
    return True


def _extract_paragraphs(children, metadata):
    paragraphs = []
    for el in children:
        if isinstance(el, str):
            continue
        if el.name == 'div':
            paragraphs.extend(_extract_paragraphs(el.children, metadata))
            if paragraphs and 'section' in (el.get('class') or []):
                break
        if el.name == 'p':
            if _is_semi_header(el):
                if len(paragraphs) == 0:
                    metadata.extend(_parse_first_header(el))
                    continue
                else:
                    break
            if _skip_paragraph_block(el, metadata):
                if len(paragraphs) > 0:
                    break
                else:
                    continue
            if el.get_text().strip():
                paragraphs.append(render(el) + '\n')
        elif el.name == 'pre':
            paragraphs.append(render(el))
        elif el.name == 'ul' and len(paragraphs) > 0:
            if _ul_is_field_list(el):
                metadata.extend(_parse_ul_field_list(el))
            else:
                paragraphs.append(
                    ''.join(
                        '* %s\n' % li.get_text()
                        for li in el.findAll('li')))
        elif re.match('h[0-9]', el.name):
            if len(paragraphs) == 0:
                if el.get_text() not in ('About', 'Introduction', 'Overview'):
                    metadata.extend(_parse_first_header(el))
                continue
            break
    return paragraphs


def _parse_field(name, body):
    if name == 'Homepage' and body.find('a'):
        yield UpstreamDatum('Homepage', body.find('a').get('href'), 'confident')
    if name == 'Home' and body.find('a'):
        yield UpstreamDatum('Homepage', body.find('a').get('href'), 'confident')
    if name == 'Issues' and body.find('a'):
        yield UpstreamDatum('Bug-Database', body.find('a').get('href'), 'confident')
    if name == 'Documentation' and body.find('a'):
        yield UpstreamDatum('Documentation', body.find('a').get('href'), 'confident')
    if name == 'License':
        yield UpstreamDatum('X-License', body.get_text(), 'confident')


def _parse_ul_field_list(el):
    for li in el.findAll('li'):
        cs = list(li.children)
        if len(cs) == 2 and isinstance(cs[0], str):
            name = cs[0].strip().rstrip(':')
            body = cs[1]
            yield from _parse_field(name, body)


def _parse_field_list(tab):
    for tr in tab.findAll('tr', {'class': 'field'}):
        name_cell = tr.find('th', {'class': 'field-name'})
        if not name_cell:
            continue
        name = name_cell.get_text().rstrip(':')
        body = tr.find('td', {'class': 'field-body'})
        if not body:
            continue
        yield from _parse_field(name, body)


def _description_from_basic_soup(soup) -> Tuple[Optional[str], Iterable[UpstreamDatum]]:
    # Drop any headers
    metadata = []
    if soup is None:
        return None, {}
    # First, skip past the first header.
    for el in soup.children:
        if el.name in ('h1', 'h2', 'h3'):
            metadata.extend(_parse_first_header(el))
            el.decompose()
            break
        elif isinstance(el, str):
            pass
        else:
            break

    table = soup.find('table', {'class': 'field-list'})
    if table:
        metadata.extend(_parse_field_list(table))

    paragraphs: List[str] = []
    paragraphs.extend(_extract_paragraphs(soup.children, metadata))

    if len(paragraphs) == 0:
        logging.debug('Empty description; no paragraphs.')
        return None, metadata

    if len(paragraphs) < 6:
        return '\n'.join(paragraphs), metadata
    logging.debug(
        'Not returning description, number of paragraphs too high: %d',
        len(paragraphs))
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
    # Strip surrogates
    html_text = html_text.encode('utf-8', 'replace').decode('utf-8')
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
    if not soup.body:
        return None, {}
    return _description_from_basic_soup(list(soup.body.children)[0])


def description_from_readme_plain(text: str) -> Tuple[Optional[str], Iterable[UpstreamDatum]]:
    lines = list(text.splitlines(False))
    metadata = []
    if not lines:
        return None, {}
    if lines[0].strip() and len(lines) > 1 and (not lines[1] or not lines[1][0].isalnum()):
        name, summary, version = _parse_first_header_text(lines[0])
        if name:
            metadata.append(UpstreamDatum('Name', name, 'likely'))
        if version:
            metadata.append(UpstreamDatum('X-Version', version, 'likely'))
        if summary:
            metadata.append(UpstreamDatum('X-Summary', summary, 'likely'))
        if name or version or summary:
            lines.pop(0)
    else:
        name = version = summary = None
    while lines and not lines[0].strip('-').strip():
        lines.pop(0)

    paras: List[List[str]] = [[]]
    for line in lines:
        if not line.strip():
            paras.append([])
        else:
            paras[-1].append(line)

    output: List[str] = []
    for para in paras:
        if not para:
            continue
        line = '\n'.join(para)
        if _skip_paragraph(line, metadata):
            continue
        output.append(line + '\n')
    if len(output) > 30:
        return None, {}
    while output and not output[-1].strip():
        output.pop(-1)
    return '\n'.join(output), metadata
