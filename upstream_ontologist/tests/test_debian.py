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

"""Tests for Debian mangling."""

from unittest import TestCase

from upstream_ontologist.debian import (
    upstream_name_to_debian_source_name,
)


class UpstreamNameToDebianSourceNameTests(TestCase):

    def test_gnu(self):
        self.assertEqual(
            'lala', upstream_name_to_debian_source_name('GNU Lala'))

    def test_parentheses(self):
        self.assertEqual(
            'mun', upstream_name_to_debian_source_name('Made Up Name (MUN)'))
