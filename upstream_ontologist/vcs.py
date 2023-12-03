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
    "plausible_url",
    "plausible_browse_url",
    "sanitize_url",
    "is_gitlab_site",
    "browse_url_from_repo_url",
    "probe_gitlab_host",
    "guess_repo_from_url",
    "probe_upstream_branch_url",
    "check_repository_url_canonical",
    "unsplit_vcs_url",
    "browse_url_from_repo_url",
    "find_public_repo_url",
    "SECURE_SCHEMES",
    "find_secure_repo_url",
    "convert_cvs_list_to_str",
    "fixup_broken_git_details",
]

from ._upstream_ontologist import (  # noqa: F401
    browse_url_from_repo_url,
    canonical_git_repo_url,
    check_repository_url_canonical,
    convert_cvs_list_to_str,
    drop_vcs_in_scheme,
    find_public_repo_url,
    guess_repo_from_url,
    is_gitlab_site,
    probe_gitlab_host,
    probe_upstream_branch_url,
    unsplit_vcs_url,
    fixup_rcp_style_git_repo_url,
    SECURE_SCHEMES,
    KNOWN_GITLAB_SITES,
    plausible_vcs_browse_url as plausible_browse_url,
    plausible_vcs_url as plausible_url,
    find_secure_repo_url,
    sanitize_url,
    fixup_broken_git_details,
)
