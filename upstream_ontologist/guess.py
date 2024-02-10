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

import logging
import os
import urllib.error
from typing import Any, Dict, Iterable, Iterator, List, Optional, Tuple, cast
from urllib.parse import urlparse
from urllib.request import Request, urlopen

from . import (
    DEFAULT_URLLIB_TIMEOUT,
    USER_AGENT,
    InvalidUrl,
    Person,
    UpstreamDatum,
    UpstreamMetadata,
    UrlUnverifiable,
    _upstream_ontologist,
)
from .vcs import (
    browse_url_from_repo_url,
    check_repository_url_canonical,
    guess_repo_from_url,
)
from .vcs import (
    sanitize_url as sanitize_vcs_url,
)

logger = logging.getLogger(__name__)


NoSuchForgeProject = _upstream_ontologist.NoSuchForgeProject


def guess_upstream_info(path, trust_package):
    return iter(_upstream_ontologist.guess_upstream_info(path, trust_package))


url_from_cvs_co_command = _upstream_ontologist.url_from_cvs_co_command
url_from_svn_co_command = _upstream_ontologist.url_from_svn_co_command
url_from_git_clone_command = _upstream_ontologist.url_from_git_clone_command
url_from_fossil_clone_command = _upstream_ontologist.url_from_fossil_clone_command
url_from_vcs_command = _upstream_ontologist.url_from_vcs_command


class NoSuchPackage(Exception):
    def __init__(self, package):
        self.package = package


GitHub = _upstream_ontologist.GitHub
GitLab = _upstream_ontologist.GitLab
SourceForge = _upstream_ontologist.SourceForge
Launchpad = _upstream_ontologist.Launchpad

find_forge = _upstream_ontologist.find_forge
repo_url_from_merge_request_url = _upstream_ontologist.repo_url_from_merge_request_url
bug_database_from_issue_url = _upstream_ontologist.bug_database_from_issue_url
guess_bug_database_url_from_repo_url = (
    _upstream_ontologist.guess_bug_database_url_from_repo_url
)
bug_database_url_from_bug_submit_url = (
    _upstream_ontologist.bug_database_url_from_bug_submit_url
)
bug_submit_url_from_bug_database_url = (
    _upstream_ontologist.bug_submit_url_from_bug_database_url
)
check_bug_database_canonical = _upstream_ontologist.check_bug_database_canonical
check_bug_submit_url_canonical = _upstream_ontologist.check_bug_submit_url_canonical
check_url_canonical = _upstream_ontologist.check_url_canonical

get_upstream_info = _upstream_ontologist.get_upstream_info

check_upstream_metadata = _upstream_ontologist.check_upstream_metadata
extend_upstream_metadata = _upstream_ontologist.extend_upstream_metadata
guess_upstream_metadata = _upstream_ontologist.guess_upstream_metadata
known_bad_guess = _upstream_ontologist.known_bad_guess


def filter_bad_guesses(
    guesses: Iterable[UpstreamDatum],
) -> Iterator[UpstreamDatum]:
    return (guess for guess in guesses if not known_bad_guess(guess))

fix_upstream_metadata = _upstream_ontologist.fix_upstream_metadata
