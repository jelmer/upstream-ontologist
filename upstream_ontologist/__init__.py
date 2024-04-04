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
- SourceForge-Project: Name of the SourceForge project
- Wiki: URL to a wiki
- Summary: A one-line description
- Description: Multi-line description
- License: Short description of the license
- Copyright
- Maintainer
- Authors

Supported, but currently not set.
- FAQ
- Donation
- Documentation
- Registration
- Webservice
"""

from dataclasses import dataclass
from email.utils import parseaddr
from typing import Optional

import ruamel.yaml

from . import _upstream_ontologist

get_upstream_info = _upstream_ontologist.get_upstream_info

SUPPORTED_CERTAINTIES = ["certain", "confident", "likely", "possible", None]

version_string = "0.1.36"

USER_AGENT = "upstream-ontologist/" + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3


yaml = ruamel.yaml.YAML(typ="safe")


@dataclass
@yaml.register_class
class Person:
    yaml_tag = "!Person"

    name: str
    email: Optional[str] = None
    url: Optional[str] = None

    def __init__(self, name, email=None, url=None):
        self.name = name
        self.email = email
        if url and url.startswith("mailto:"):
            self.email = url[len("mailto:") :]
            self.url = None
        else:
            self.url = url

    @classmethod
    def from_yaml(cls, constructor, node):
        d = {}
        for k, v in node.value:
            d[k.value] = v.value
        return cls(name=d.get("name"), email=d.get("email"), url=d.get("url"))

    @classmethod
    def from_string(cls, text):
        text = text.replace(" at ", "@")
        text = text.replace(" -at- ", "@")
        text = text.replace(" -dot- ", ".")
        text = text.replace("[AT]", "@")
        if "(" in text and text.endswith(")"):
            (p1, p2) = text[:-1].split("(", 1)
            if p2.startswith("https://") or p2.startswith("http://"):
                url = p2
                if "<" in p1:
                    (name, email) = parseaddr(p1)
                    return cls(name=name, email=email, url=url)
                return cls(name=p1, url=url)
            elif "@" in p2:
                return cls(name=p1, email=p2)
            return cls(text)
        elif "<" in text:
            (name, email) = parseaddr(text)
            return cls(name=name, email=email)
        else:
            return cls(name=text)

    def __str__(self):
        if self.email:
            return f"{self.name} <{self.email}>"
        return self.name


UpstreamDatum = _upstream_ontologist.UpstreamDatum
UpstreamMetadata = _upstream_ontologist.UpstreamMetadata


class UpstreamPackage:
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


class UrlUnverifiable(Exception):
    """Unable to check specified URL."""

    def __init__(self, url, reason):
        self.url = url
        self.reason = reason


class InvalidUrl(Exception):
    """Specified URL is invalid."""

    def __init__(self, url, reason):
        self.url = url
        self.reason = reason
