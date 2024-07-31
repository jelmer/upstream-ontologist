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

"""README parsing."""

import logging
import platform
import re
from collections.abc import Iterable
from typing import Optional
from urllib.parse import urlparse

from . import UpstreamDatum, _upstream_ontologist

logger = logging.getLogger(__name__)


description_from_readme_md = _upstream_ontologist.description_from_readme_md  # type: ignore
description_from_readme_plain = _upstream_ontologist.description_from_readme_plain  # type: ignore
description_from_readme_rst = _upstream_ontologist.description_from_readme_rst  # type: ignore
description_from_readme_html = _upstream_ontologist.description_from_readme_html  # type: ignore
