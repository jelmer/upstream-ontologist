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
- X-SourceForge-Project
- X-Wiki
- X-Summary
- X-Description
- X-License
- X-Copyright

Supported, but currently not set.
- FAQ
- Donation
- Documentation
- Registration
- Webservice
"""

from typing import Optional, Sequence

SUPPORTED_CERTAINTIES = ['certain', 'confident', 'likely', 'possible', None]

version_string = '0.1.7'

USER_AGENT = 'upstream-ontologist/' + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3


class UpstreamDatum(object):
    """A single piece of upstream metadata."""

    __slots__ = ['field', 'value', 'certainty', 'origin']

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
        return isinstance(other, type(self)) and \
                self.field == other.field and \
                self.value == other.value and \
                self.certainty == other.certainty and \
                self.origin == other.origin

    def __str__(self):
        return "%s: %s" % (self.field, self.value)

    def __repr__(self):
        return "%s(%r, %r, %r, %r)" % (
            type(self).__name__, self.field, self.value, self.certainty,
            self.origin)


class UpstreamPackage(object):

    def __init__(self, family, name):
        self.family = family
        self.name = name


class UpstreamRequirement(object):
    """Upstream dependency."""

    def __init__(self, stage, kind, name, origin=None):
        self.stage = stage
        self.package = UpstreamPackage(kind, name)
        self.origin = origin

    @property
    def kind(self):
        return self.package.family

    @property
    def name(self):
        return self.package.name

    def __str__(self):
        return "%s Upstream Requirement (%s): %s" % (
            self.stage, self.kind, self.name)

    def __repr__(self):
        return "%s(%r, %r, %r, origin=%r)" % (
            type(self).__name__, self.stage, self.kind, self.name, self.origin)

    def __eq__(self, other):
        return (
            isinstance(other, type(self)) and self.stage == other.stage and
            self.kind == other.kind and self.name == other.name and
            self.origin == other.origin)


class UpstreamOutput(object):
    """Upstream output."""

    def __init__(self, kind, name, origin=None):
        self.kind = kind
        self.name = name
        self.origin = origin

    def __str__(self):
        return "%s: %s" % (self.kind, self.name)

    def __repr__(self):
        return "%s(%r, %r, origin=%r)" % (
            type(self).__name__, self.kind, self.name, self.origin)

    def __eq__(self, other):
        return (
            isinstance(other, type(self)) and self.kind == other.kind and
            self.name == other.name and self.origin == other.origin)


class BuildSystem(object):
    """A build system for an upstream."""

    def __init__(self, name, origin=None):
        self.name = name
        self.origin = origin

    def __str__(self):
        return self.name

    def __repr__(self):
        return "%s(%r, origin=%r)" % (
            type(self).__name__, self.name, self.origin)

    def __eq__(self, other):
        return (
            isinstance(other, type(self)) and
            other.name == self.name and
            other.origin == self.origin)


# If we're setting them new, put Name and Contact first
def upstream_metadata_sort_key(x):
    (k, v) = x
    return {
        'Name': '00-Name',
        'Contact': '01-Contact',
        }.get(k, k)


def min_certainty(certainties: Sequence[str]) -> str:
    return confidence_to_certainty(
        max([certainty_to_confidence(c)
            for c in certainties] + [0]))


def certainty_to_confidence(certainty: Optional[str]) -> Optional[int]:
    if certainty in ('unknown', None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


def confidence_to_certainty(confidence: Optional[int]) -> str:
    if confidence is None:
        return 'unknown'
    try:
        return SUPPORTED_CERTAINTIES[confidence] or 'unknown'
    except IndexError:
        raise ValueError(confidence)


def certainty_sufficient(actual_certainty: str,
                         minimum_certainty: Optional[str]) -> bool:
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
