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

import os
import unittest

from upstream_ontologist.guess import get_upstream_info
from upstream_ontologist import yaml


class TestDataTestCase(unittest.TestCase):
    """Test case that runs a fixer test."""

    def __init__(self, name, path):
        self.name = name
        self.path = path
        self.maxDiff = None
        super().__init__()

    def id(self):
        return f"{__name__}.{self.name}"

    def __str__(self):
        return f"testdata test: {self.name}"

    def runTest(self):
        got = get_upstream_info(self.path, trust_package=True, net_access=False, check=False)
        jp = os.path.join(self.path, 'expected.yaml')
        with open(jp, 'r') as f:
            expected = yaml.load(f)
        self.assertEqual(expected, got)


def test_suite():
    suite = unittest.TestSuite()
    for entry in os.scandir(os.path.join(os.path.dirname(__file__), "..", "testdata")):
        if entry.name.endswith('~'):
            continue
        suite.addTest(TestDataTestCase(entry.name, entry.path))
    return suite
