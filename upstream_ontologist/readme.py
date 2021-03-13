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


def description_from_readme_md(md_text):
    """Description from README.md."""
    import markdown
    html_text = markdown.markdown(md_text)
    from bs4 import BeautifulSoup, NavigableString
    soup = BeautifulSoup(html_text, 'lxml')
    first = next(iter(soup.body.children))
    if first.name == 'h1':
        first.decompose()
    paragraphs = []
    for el in soup.body.findAll(['p', 'ul']):
        if el.name == 'p':
            paragraphs.append(el.get_text() + '\n')
        elif el.name == 'ul':
            paragraphs.append(
                ''.join(
                    '* %s\n' % li.get_text()
                    for li in el.findAll('li')))
    return '\n'.join(paragraphs)
