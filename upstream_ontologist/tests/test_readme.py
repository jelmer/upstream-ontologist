#!/usr/bin/python
# Copyright (C) 2019 Jelmer Vernooij
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

"""Tests for readme parsing."""

from unittest import TestCase

from upstream_ontologist.readme import (
    description_from_readme_md,
    )


class ReadmeTests(TestCase):

    def setUp(self):
        super(ReadmeTests, self).setUp()
        self.maxDiff = None

    def test_sfcgal(self):
        self.assertEqual("""\
SFCGAL is a C++ wrapper library around CGAL with the aim \
of supporting ISO 191007:2013 and OGC Simple Features for 3D operations.

Please refer to the project page for an updated installation procedure.
""", description_from_readme_md("""\
SFCGAL
======

SFCGAL is a C++ wrapper library around \
[CGAL](http://www.cgal.org) with the aim \
of supporting ISO 191007:2013 and OGC Simple Features for 3D operations.

Please refer to the \
<a href="http://oslandia.github.io/SFCGAL">project page</a> \
for an updated installation procedure."""))

    def test_erbium(self):
        self.assertEqual("""\
Erbium[^0] provides networking services for use on small/home networks.  Erbium
currently supports both DNS and DHCP, with other protocols hopefully coming soon.

Erbium is in early development.

* DNS is still in early development, and not ready for use.
* DHCP is beta quality.  Should be ready for test use.
* Router Advertisements are alpha quality.  Should be ready for limited testing.

[^0]: Erbium is the 68th element in the periodic table, the same as the client
port number for DHCP.
""", description_from_readme_md("""\
Erbium
======

Erbium[^0] provides networking services for use on small/home networks.  Erbium
currently supports both DNS and DHCP, with other protocols hopefully coming soon.

Erbium is in early development.

   * DNS is still in early development, and not ready for use.
   * DHCP is beta quality.  Should be ready for test use.
   * Router Advertisements are alpha quality.  Should be ready for limited testing.



[^0]: Erbium is the 68th element in the periodic table, the same as the client
port number for DHCP.
"""))
