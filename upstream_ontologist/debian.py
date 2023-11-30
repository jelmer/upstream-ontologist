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

from .. import UpstreamPackage, _upstream_ontologist


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
    return package.name.lower().replace("_", "-")


def compare_upstream_versions(family, version1, version2):
    raise NotImplementedError


valid_debian_package_name = _upstream_ontologist.valid_debian_package_name
debian_to_upstream_version = _upstream_ontologist.debian_to_upstream_version
upstream_name_to_debian_source_name = _upstream_ontologist.upstream_name_to_debian_source_name
debian_to_upstream_version = _upstream_ontologist.debian_to_upstream_version
upstream_name_to_debian_source_name = _upstream_ontologist.upstream_name_to_debian_source_name
upstream_version_to_debian_upstream_version = _upstream_ontologist.upstream_version_to_debian_upstream_version
