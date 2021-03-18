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

import os
import platform
from unittest import TestCase, TestSuite

from upstream_ontologist.readme import (
    description_from_readme_md,
    description_from_readme_rst,
    )


class ReadmeTestCase(TestCase):

    def __init__(self, path):
        super(ReadmeTestCase, self).__init__()
        self.path = path

    def setUp(self):
        super(ReadmeTestCase, self).setUp()
        self.maxDiff = None

    def runTest(self):
        readme_md = None
        readme_rst = None
        description = None
        for entry in os.scandir(self.path):
            if entry.name.endswith('~'):
                continue
            base, ext = os.path.splitext(entry.name)
            if entry.name == 'description':
                with open(entry.path, 'r') as f:
                    description = f.read()
            elif base == "README":
                if ext == '.md':
                    with open(entry.path, 'r') as f:
                        readme_md = f.read()
                elif ext == '.rst':
                    with open(entry.path, 'r') as f:
                        readme_rst = f.read()
                else:
                    raise NotImplementedError(ext)
            else:
                raise NotImplementedError(ext)

        if readme_md is not None:
            try:
                import markdown  # noqa: F401
            except ModuleNotFoundError:
                self.skipTest(
                    'Skipping README.md tests, markdown not available')
            actual_description, unused_md = description_from_readme_md(
                readme_md)
            self.assertEqual(actual_description, description)

        if readme_rst is not None:
            if platform.python_implementation() == "PyPy":
                self.skipTest('Skipping README.rst tests on pypy')
            try:
                import docutils  # noqa: F401
            except ModuleNotFoundError:
                self.skipTest(
                    'Skipping README.rst tests, docutils not available')
            actual_description, unused_rst = description_from_readme_rst(
                    readme_rst)
            self.assertEqual(actual_description, description)


def test_suite():
    suite = TestSuite()
    for entry in os.scandir(os.path.join(os.path.dirname(__file__), 'readme_data')):
        suite.addTest(ReadmeTestCase(entry.path))
    return suite
