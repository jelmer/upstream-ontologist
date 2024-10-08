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


from unittest import TestCase

from upstream_ontologist.vcs import (
    canonical_git_repo_url,
    find_public_repo_url,
    fixup_rcp_style_git_repo_url,
)


class CanonicalizeVcsUrlTests(TestCase):
    def test_github(self):
        self.assertEqual(
            "https://github.com/jelmer/example.git",
            canonical_git_repo_url("https://github.com/jelmer/example"),
        )

    def test_salsa(self):
        self.assertEqual(
            "https://salsa.debian.org/jelmer/example.git",
            canonical_git_repo_url("https://salsa.debian.org/jelmer/example"),
        )
        self.assertEqual(
            "https://salsa.debian.org/jelmer/example.git",
            canonical_git_repo_url("https://salsa.debian.org/jelmer/example.git"),
        )


class FindPublicVcsUrlTests(TestCase):
    def test_github(self):
        self.assertEqual(
            "https://github.com/jelmer/example",
            find_public_repo_url("ssh://git@github.com/jelmer/example"),
        )
        self.assertEqual(
            "https://github.com/jelmer/example",
            find_public_repo_url("https://github.com/jelmer/example"),
        )
        self.assertEqual(
            "https://github.com/jelmer/example",
            find_public_repo_url("git@github.com:jelmer/example"),
        )

    def test_salsa(self):
        self.assertEqual(
            "https://salsa.debian.org/jelmer/example",
            find_public_repo_url("ssh://salsa.debian.org/jelmer/example"),
        )
        self.assertEqual(
            "https://salsa.debian.org/jelmer/example",
            find_public_repo_url("https://salsa.debian.org/jelmer/example"),
        )


class FixupRcpStyleUrlTests(TestCase):
    def test_fixup(self):
        try:
            import breezy  # noqa: F401
        except ModuleNotFoundError:
            self.skipTest("breezy is not available")
        self.assertEqual(
            "ssh://github.com/jelmer/example",
            fixup_rcp_style_git_repo_url("github.com:jelmer/example"),
        )
        self.assertEqual(
            "ssh://git@github.com/jelmer/example",
            fixup_rcp_style_git_repo_url("git@github.com:jelmer/example"),
        )

    def test_leave(self):
        try:
            import breezy  # noqa: F401
        except ModuleNotFoundError:
            self.skipTest("breezy is not available")
        self.assertEqual(
            "https://salsa.debian.org/jelmer/example",
            fixup_rcp_style_git_repo_url("https://salsa.debian.org/jelmer/example"),
        )
        self.assertEqual(
            "ssh://git@salsa.debian.org/jelmer/example",
            fixup_rcp_style_git_repo_url("ssh://git@salsa.debian.org/jelmer/example"),
        )
