#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij
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

"""Tests for the vcs module."""

from unittest import TestCase

from lintian_brush.vcs import (
    is_gitlab_site,
    )


class TestIsGitLabSite(TestCase):

    def test_not_gitlab(self):
        self.assertFalse(is_gitlab_site('foo.example.com'))
        self.assertFalse(is_gitlab_site('github.com'))
        self.assertFalse(is_gitlab_site(None))

    def test_gitlab(self):
        self.assertTrue(is_gitlab_site('gitlab.somehost.com'))
        self.assertTrue(is_gitlab_site('salsa.debian.org'))
