#!/usr/bin/python
# Copyright (C) 2022 Jelmer Vernooij
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

"""Tests for guess."""

from unittest import TestCase

from upstream_ontologist import UpstreamDatum, Person
from upstream_ontologist.guess import (
    metadata_from_itp_bug_body
)


class MetadataFromItpBugBody(TestCase):

    def test_simple(self):
        self.assertEqual([
            UpstreamDatum('Name', 'setuptools-gettext', 'confident'),
            UpstreamDatum('X-Version', '0.0.1', 'possible'),
            UpstreamDatum(
                'X-Author', [Person.from_string(
                    'Breezy Team <breezy-core@googlegroups.com>')], 'confident'),
            UpstreamDatum('Homepage', 'https://github.com/jelmer/setuptools-gettext', 'confident'),
            UpstreamDatum('X-License', 'GPL', 'confident'),
            UpstreamDatum('X-Summary', 'Compile .po files into .mo files', 'confident'),
            UpstreamDatum('X-Description', """\
This extension for setuptools compiles gettext .po files
found in the source directory into .mo files and installs them.
""", 'likely')
        ], list(metadata_from_itp_bug_body("""\
Package: wnpp
Severity: wishlist
Owner: Jelmer Vernooij <jelmer@debian.org>
X-Debbugs-Cc: debian-devel@lists.debian.org

* Package name    : setuptools-gettext
  Version         : 0.0.1
  Upstream Author : Breezy Team <breezy-core@googlegroups.com>
* URL             : https://github.com/jelmer/setuptools-gettext
* License         : GPL
  Programming Lang: Python
  Description     : Compile .po files into .mo files

This extension for setuptools compiles gettext .po files
found in the source directory into .mo files and installs them.

""")))
