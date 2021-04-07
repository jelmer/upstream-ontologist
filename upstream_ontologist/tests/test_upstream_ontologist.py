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
    UpstreamDatum,
    min_certainty,
    certainty_to_confidence,
    confidence_to_certainty,
    certainty_sufficient,
)
from upstream_ontologist.guess import (
    guess_repo_from_url,
    guess_from_package_json,
    guess_from_debian_watch,
    guess_from_r_description,
    bug_database_url_from_bug_submit_url,
    url_from_git_clone_command,
    url_from_fossil_clone_command,
)


class TestCaseInTempDir(TestCase):
    def setUp(self):
        super(TestCaseInTempDir, self).setUp()
        self.testdir = tempfile.mkdtemp()
        os.chdir(self.testdir)
        self.addCleanup(shutil.rmtree, self.testdir)


class GuessFromDebianWatchTests(TestCaseInTempDir):
    def test_empty(self):
        with open("watch", "w") as f:
            f.write(
                """\
# Blah
"""
            )
        self.assertEqual([], list(guess_from_debian_watch("watch", False)))

    def test_simple(self):
        with open("watch", "w") as f:
            f.write(
                """\
version=4
https://github.com/jelmer/dulwich/tags/dulwich-(.*).tar.gz
"""
            )
        self.assertEqual(
            [
                UpstreamDatum(
                    "Repository", "https://github.com/jelmer/dulwich", "likely", "watch"
                )
            ],
            list(guess_from_debian_watch("watch", False)),
        )


class GuessFromPackageJsonTests(TestCaseInTempDir):
    def test_simple(self):
        with open("package.json", "w") as f:
            f.write(
                """\
{
  "name": "autosize",
  "version": "4.0.2",
  "author": {
    "name": "Jack Moore",
    "url": "http://www.jacklmoore.com",
    "email": "hello@jacklmoore.com"
  },
  "main": "dist/autosize.js",
  "license": "MIT",
  "homepage": "http://www.jacklmoore.com/autosize",
  "demo": "http://www.jacklmoore.com/autosize",
  "repository": {
    "type": "git",
    "url": "http://github.com/jackmoore/autosize.git"
  }
}
"""
            )
        self.assertEqual(
            [
                UpstreamDatum("Name", "autosize", "certain"),
                UpstreamDatum(
                    "Homepage", "http://www.jacklmoore.com/autosize", "certain"
                ),
                UpstreamDatum("X-License", "MIT", "certain", None),
                UpstreamDatum("X-Version", "4.0.2", "certain"),
                UpstreamDatum(
                    "Repository", "http://github.com/jackmoore/autosize.git", "certain"
                ),
            ],
            list(guess_from_package_json("package.json", False)),
        )

    def test_dummy(self):
        with open("package.json", "w") as f:
            f.write(
                """\
{
  "name": "mozillaeslintsetup",
  "description": "This package file is for setup of ESLint.",
  "repository": {},
  "license": "MPL-2.0",
  "dependencies": {
    "eslint": "4.18.1",
    "eslint-plugin-html": "4.0.2",
    "eslint-plugin-mozilla": "file:tools/lint/eslint/eslint-plugin-mozilla",
    "eslint-plugin-no-unsanitized": "2.0.2",
    "eslint-plugin-react": "7.1.0",
    "eslint-plugin-spidermonkey-js":
        "file:tools/lint/eslint/eslint-plugin-spidermonkey-js"
  },
  "devDependencies": {}
}
"""
            )
        self.assertEqual(
            [
                UpstreamDatum("Name", "mozillaeslintsetup", "certain"),
                UpstreamDatum(
                    "X-Summary",
                    "This package file is for setup of ESLint.",
                    "certain",
                    None,
                ),
                UpstreamDatum("X-License", "MPL-2.0", "certain", None),
            ],
            list(guess_from_package_json("package.json", False)),
        )


class GuessFromRDescriptionTests(TestCaseInTempDir):
    def test_read(self):
        with open("DESCRIPTION", "w") as f:
            f.write(
                """\
Package: crul
Title: HTTP Client
Description: A simple HTTP client, with tools for making HTTP requests,
    and mocking HTTP requests. The package is built on R6, and takes
    inspiration from Ruby's 'faraday' gem (<https://rubygems.org/gems/faraday>)
    The package name is a play on curl, the widely used command line tool
    for HTTP, and this package is built on top of the R package 'curl', an
    interface to 'libcurl' (<https://curl.haxx.se/libcurl>).
Version: 0.8.4
License: MIT + file LICENSE
Authors@R: c(
    person("Scott", "Chamberlain", role = c("aut", "cre"),
    email = "myrmecocystus@gmail.com",
    comment = c(ORCID = "0000-0003-1444-9135"))
    )
URL: https://github.com/ropensci/crul (devel)
        https://ropenscilabs.github.io/http-testing-book/ (user manual)
        https://www.example.com/crul (homepage)
BugReports: https://github.com/ropensci/crul/issues
Encoding: UTF-8
Language: en-US
Imports: curl (>= 3.3), R6 (>= 2.2.0), urltools (>= 1.6.0), httpcode
        (>= 0.2.0), jsonlite, mime
Suggests: testthat, fauxpas (>= 0.1.0), webmockr (>= 0.1.0), knitr
VignetteBuilder: knitr
RoxygenNote: 6.1.1
X-schema.org-applicationCategory: Web
X-schema.org-keywords: http, https, API, web-services, curl, download,
        libcurl, async, mocking, caching
X-schema.org-isPartOf: https://ropensci.org
NeedsCompilation: no
Packaged: 2019-08-02 19:58:21 UTC; sckott
Author: Scott Chamberlain [aut, cre] (<https://orcid.org/0000-0003-1444-9135>)
Maintainer: Scott Chamberlain <myrmecocystus@gmail.com>
Repository: CRAN
Date/Publication: 2019-08-02 20:30:02 UTC
"""
            )
        ret = guess_from_r_description("DESCRIPTION", True)
        self.assertEqual(
            list(ret),
            [
                UpstreamDatum("Name", "crul", "certain"),
                UpstreamDatum("Archive", "CRAN", "certain"),
                UpstreamDatum(
                    "Bug-Database", "https://github.com/ropensci/crul/issues", "certain"
                ),
                UpstreamDatum('X-Version', '0.8.4', 'certain'),
                UpstreamDatum('X-License', 'MIT + file LICENSE', 'certain'),
                UpstreamDatum('X-Summary', 'HTTP Client', 'certain'),
                UpstreamDatum('X-Description', """\
A simple HTTP client, with tools for making HTTP requests,
and mocking HTTP requests. The package is built on R6, and takes
inspiration from Ruby's 'faraday' gem (<https://rubygems.org/gems/faraday>)
The package name is a play on curl, the widely used command line tool
for HTTP, and this package is built on top of the R package 'curl', an
interface to 'libcurl' (<https://curl.haxx.se/libcurl>).""", 'certain'),
                UpstreamDatum('Contact', 'Scott Chamberlain <myrmecocystus@gmail.com>', 'certain'),
                UpstreamDatum(
                    "Repository", "https://github.com/ropensci/crul", "certain"
                ),
                UpstreamDatum("Homepage", "https://www.example.com/crul", "certain"),
            ],
        )


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
