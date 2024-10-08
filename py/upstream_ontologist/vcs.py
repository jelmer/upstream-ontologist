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

__all__ = [
    "find_public_repo_url",
    "find_secure_repo_url",
    "convert_cvs_list_to_str",
    "fixup_broken_git_details",
]

from ._upstream_ontologist import (  # noqa: F401
    canonical_git_repo_url,
    convert_cvs_list_to_str,
    drop_vcs_in_scheme,
    find_public_repo_url,
    find_secure_repo_url,
    fixup_broken_git_details,
    fixup_rcp_style_git_repo_url,
)
