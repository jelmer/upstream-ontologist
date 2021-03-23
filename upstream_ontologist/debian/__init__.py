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

import re
from typing import Optional

from .. import UpstreamPackage


def debian_to_upstream_version(version):
    """Drop debian-specific modifiers from an upstream version string."""
    return version.upstream_version.split("+dfsg")[0]


def upstream_name_to_debian_source_name(upstream_name: str) -> str:
    if upstream_name.startswith("GNU "):
        upstream_name = upstream_name[len("GNU ") :]
    return upstream_name.lower().replace('_', '-').replace(' ', '-').replace('/', '-')


def upstream_version_to_debian_upstream_version(
    version: str, family: Optional[str] = None
) -> str:
    # TODO(jelmer)
    return version


def upstream_package_to_debian_source_name(package: UpstreamPackage) -> str:
    if package.family == "rust":
        return "rust-%s" % package.name.lower()
    if package.family == "perl":
        return "lib%s-perl" % package.name.lower().replace("::", "-")
    if package.family == "node":
        return "node-%s" % package.name.lower()
    # TODO(jelmer):
    return upstream_name_to_debian_source_name(package.name)


def upstream_package_to_debian_binary_name(package: UpstreamPackage) -> str:
    if package.family == "rust":
        return "rust-%s" % package.name.lower()
    if package.family == "perl":
        return "lib%s-perl" % package.name.lower().replace("::", "-")
    if package.family == "node":
        return "node-%s" % package.name.lower()
    # TODO(jelmer):
    return package.name.lower().replace('_', '-')


def compare_upstream_versions(family, version1, version2):
    raise NotImplementedError


package_name_re = re.compile("[a-z0-9][a-z0-9+-.]+")


def valid_debian_package_name(name):
    return bool(package_name_re.fullmatch(name))
