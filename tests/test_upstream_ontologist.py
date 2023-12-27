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

"""Tests for upstream_ontologist."""

import os
import shutil
import tempfile
from unittest import (
    TestCase,
)

from upstream_ontologist.guess import (
    bug_database_url_from_bug_submit_url,
    guess_repo_from_url,
    url_from_fossil_clone_command,
    url_from_git_clone_command,
)


class TestCaseInTempDir(TestCase):
    def setUp(self):
        super().setUp()
        self.testdir = tempfile.mkdtemp()
        os.chdir(self.testdir)
        self.addCleanup(shutil.rmtree, self.testdir)


class GuessRepoFromUrlTests(TestCase):
    def test_github(self):
        self.assertEqual(
            "https://github.com/jelmer/blah",
            guess_repo_from_url("https://github.com/jelmer/blah"),
        )
        self.assertEqual(
            "https://github.com/jelmer/blah",
            guess_repo_from_url("https://github.com/jelmer/blah/blob/README"),
        )
        self.assertIs(None, guess_repo_from_url("https://github.com/jelmer"))

    def test_none(self):
        self.assertIs(None, guess_repo_from_url("https://www.jelmer.uk/"))

    def test_known(self):
        self.assertEqual(
            "http://code.launchpad.net/blah",
            guess_repo_from_url("http://code.launchpad.net/blah"),
        )

    def test_launchpad(self):
        self.assertEqual(
            "https://code.launchpad.net/bzr",
            guess_repo_from_url("http://launchpad.net/bzr/+download"),
        )

    def test_savannah(self):
        self.assertEqual(
            "https://git.savannah.gnu.org/git/auctex.git",
            guess_repo_from_url("https://git.savannah.gnu.org/git/auctex.git"),
        )
        self.assertIs(
            None, guess_repo_from_url("https://git.savannah.gnu.org/blah/auctex.git")
        )

    def test_bitbucket(self):
        self.assertEqual(
            "https://bitbucket.org/fenics-project/dolfin",
            guess_repo_from_url(
                "https://bitbucket.org/fenics-project/dolfin/downloads/"
            ),
        )


class BugDbFromBugSubmitUrlTests(TestCase):
    def test_github(self):
        self.assertEqual(
            "https://github.com/dulwich/dulwich/issues",
            bug_database_url_from_bug_submit_url(
                "https://github.com/dulwich/dulwich/issues/new"
            ),
        )

    def test_sf(self):
        self.assertEqual(
            "https://sourceforge.net/p/dulwich/bugs",
            bug_database_url_from_bug_submit_url(
                "https://sourceforge.net/p/dulwich/bugs/new"
            ),
        )


class UrlFromGitCloneTests(TestCase):
    def test_guess_simple(self):
        self.assertEqual(
            "https://github.com/jelmer/blah",
            url_from_git_clone_command(b"git clone https://github.com/jelmer/blah"),
        )
        self.assertEqual(
            "https://github.com/jelmer/blah",
            url_from_git_clone_command(
                b"git clone https://github.com/jelmer/blah target"
            ),
        )

    def test_args(self):
        self.assertEqual(
            "https://github.com/jelmer/blah",
            url_from_git_clone_command(
                b"git clone -b foo https://github.com/jelmer/blah target"
            ),
        )


class UrlFromFossilCloneTests(TestCase):
    def test_guess_simple(self):
        self.assertEqual(
            "https://example.com/repo/blah",
            url_from_fossil_clone_command(
                b"fossil clone https://example.com/repo/blah blah.fossil"
            ),
        )
