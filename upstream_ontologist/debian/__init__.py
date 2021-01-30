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


def debian_to_upstream_version(version):
    """Drop debian-specific modifiers from an upstream version string.
    """
    return version.upstream_version.split("+dfsg")[0]


def upstream_name_to_debian_source_name(upstream_name: str) -> str:
    if upstream_name.startswith('GNU '):
        upstream_name = upstream_name[len('GNU '):]
    return upstream_name.lower()


def upstream_version_to_debian_upstream_version(version: str) -> str:
    # TODO(jelmer)
    return version
