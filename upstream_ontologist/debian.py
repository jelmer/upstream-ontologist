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

from . import _upstream_ontologist

upstream_package_to_debian_source_name = _upstream_ontologist.debian.upstream_package_to_debian_source_name  # type: ignore
upstream_package_to_debian_binary_name = _upstream_ontologist.debian.upstream_package_to_debian_binary_name  # type: ignore
valid_debian_package_name = _upstream_ontologist.debian.valid_debian_package_name  # type: ignore
debian_to_upstream_version = _upstream_ontologist.debian.debian_to_upstream_version  # type: ignore
upstream_name_to_debian_source_name = _upstream_ontologist.debian.upstream_name_to_debian_source_name  # type: ignore
