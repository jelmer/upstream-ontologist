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

from upstream_ontologist import (
    certainty_sufficient,
    certainty_to_confidence,
    confidence_to_certainty,
    min_certainty,
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


class CertaintySufficientTests(TestCase):
    def test_sufficient(self):
        self.assertTrue(certainty_sufficient("certain", "certain"))
        self.assertTrue(certainty_sufficient("certain", "possible"))
        self.assertTrue(certainty_sufficient("certain", None))
        self.assertTrue(certainty_sufficient("possible", None))
        # TODO(jelmer): Should we really always allow unknown certainties
        # through?
        self.assertTrue(certainty_sufficient(None, "certain"))

    def test_insufficient(self):
        self.assertFalse(certainty_sufficient("possible", "certain"))


class CertaintyVsConfidenceTests(TestCase):
    def test_confidence_to_certainty(self):
        self.assertEqual("certain", confidence_to_certainty(0))
        self.assertEqual("confident", confidence_to_certainty(1))
        self.assertEqual("likely", confidence_to_certainty(2))
        self.assertEqual("possible", confidence_to_certainty(3))
        self.assertEqual("unknown", confidence_to_certainty(None))
        self.assertRaises(ValueError, confidence_to_certainty, 2000)

    def test_certainty_to_confidence(self):
        self.assertEqual(0, certainty_to_confidence("certain"))
        self.assertEqual(1, certainty_to_confidence("confident"))
        self.assertEqual(2, certainty_to_confidence("likely"))
        self.assertEqual(3, certainty_to_confidence("possible"))
        self.assertIs(None, certainty_to_confidence("unknown"))
        self.assertRaises(ValueError, certainty_to_confidence, "blah")


class MinimumCertaintyTests(TestCase):
    def test_minimum(self):
        self.assertEqual("certain", min_certainty([]))
        self.assertEqual("certain", min_certainty(["certain"]))
        self.assertEqual("possible", min_certainty(["possible"]))
        self.assertEqual("possible", min_certainty(["possible", "certain"]))
        self.assertEqual("likely", min_certainty(["likely", "certain"]))
        self.assertEqual("possible", min_certainty(["likely", "certain", "possible"]))
