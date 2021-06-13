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

"""Functions for working with upstream metadata.

This gathers information about upstreams from various places.
Each bit of information gathered is wrapped in a UpstreamDatum
object, which contains the field name.

The fields used here match those in https://wiki.debian.org/UpstreamMetadata

Supported fields:
- Homepage
- Name
- Contact
- Repository
- Repository-Browse
- Bug-Database
- Bug-Submit
- Screenshots
- Archive
- Security-Contact

Extensions for upstream-ontologist.
- X-SourceForge-Project: Name of the SourceForge project
- X-Wiki: URL to a wiki
- X-Summary: A one-line description
- X-Description: Multi-line description
- X-License: Short description of the license
- X-Copyright
- X-Maintainer
- X-Authors

Supported, but currently not set.
- FAQ
- Donation
- Documentation
- Registration
- Webservice
"""

from typing import Optional, Sequence
from dataclasses import dataclass
from email.utils import parseaddr


SUPPORTED_CERTAINTIES = ["certain", "confident", "likely", "possible", None]

version_string = "0.1.22"

USER_AGENT = "upstream-ontologist/" + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3


@dataclass
class Person:

    name: str
    email: Optional[str] = None

    @classmethod
    def from_string(cls, text):
        text = text.replace(' at ', '@')
        text = text.replace('[AT]', '@')
        if '<' in text:
            (name, email) = parseaddr(text)
            return cls(name=name, email=email)
        else:
            return cls(name=text)

    def __str__(self):
        if self.email:
            return '%s <%s>' % (self.name, self.email)
        return self.name


class UpstreamDatum(object):
    """A single piece of upstream metadata."""

    __slots__ = ["field", "value", "certainty", "origin"]

    def __init__(self, field, value, certainty=None, origin=None):
        self.field = field
        if value is None:
            raise ValueError(field)
        self.value = value
        if certainty not in SUPPORTED_CERTAINTIES:
            raise ValueError(certainty)
        self.certainty = certainty
        self.origin = origin

    def __eq__(self, other):
        return (
            isinstance(other, type(self))
            and self.field == other.field
            and self.value == other.value
            and self.certainty == other.certainty
            and self.origin == other.origin
        )

    def __str__(self):
        return "%s: %s" % (self.field, self.value)

    def __repr__(self):
        return "%s(%r, %r, %r, %r)" % (
            type(self).__name__,
            self.field,
            self.value,
            self.certainty,
            self.origin,
        )


class UpstreamPackage(object):
    def __init__(self, family, name):
        self.family = family
        self.name = name


# If we're setting them new, put Name and Contact first
def upstream_metadata_sort_key(x):
    (k, v) = x
    return {
        "Name": "00-Name",
        "Contact": "01-Contact",
    }.get(k, k)


def min_certainty(certainties: Sequence[str]) -> str:
    confidences = [certainty_to_confidence(c) for c in certainties]
    return confidence_to_certainty(max([c for c in confidences if c is not None] + [0]))


def certainty_to_confidence(certainty: Optional[str]) -> Optional[int]:
    if certainty in ("unknown", None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


def confidence_to_certainty(confidence: Optional[int]) -> str:
    if confidence is None:
        return "unknown"
    try:
        return SUPPORTED_CERTAINTIES[confidence] or "unknown"
    except IndexError:
        raise ValueError(confidence)


def certainty_sufficient(
    actual_certainty: str, minimum_certainty: Optional[str]
) -> bool:
    """Check if the actual certainty is sufficient.

    Args:
      actual_certainty: Actual certainty with which changes were made
      minimum_certainty: Minimum certainty to keep changes
    Returns:
      boolean
    """
    actual_confidence = certainty_to_confidence(actual_certainty)
    if actual_confidence is None:
        # Actual confidence is unknown.
        # TODO(jelmer): Should we really be ignoring this?
        return True
    minimum_confidence = certainty_to_confidence(minimum_certainty)
    if minimum_confidence is None:
        return True
    return actual_confidence <= minimum_confidence


def _load_json_url(http_url: str, timeout: int = DEFAULT_URLLIB_TIMEOUT):
    from urllib.request import urlopen, Request
    import json
    headers = {'User-Agent': USER_AGENT, 'Accept': 'application/json'}
    http_contents = urlopen(
        Request(http_url, headers=headers),
        timeout=timeout).read()
    return json.loads(http_contents)
