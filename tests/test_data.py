#!/usr/bin/python3
# Copyright (C) 2024 Jelmer Vernooij <jelmer@debian.org>
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


from typing import Any
from unittest import TestCase

from upstream_ontologist import UpstreamMetadata


class UpstreamMetadataFromDictTests(TestCase):
    def test_from_dict(self):
        d = {
            "Name": "foo",
            "Version": "1.2.3",
            "Homepage": "https://example.com",
        }
        metadata = UpstreamMetadata.from_dict(d)
        self.assertEqual(metadata["Name"].value, "foo")
        self.assertEqual(metadata["Version"].value, "1.2.3")
        self.assertEqual(metadata["Homepage"].value, "https://example.com")

    def test_from_dict_missing(self):
        d = {
            "Name": "foo",
            "Version": "1.2.3",
        }
        metadata = UpstreamMetadata.from_dict(d)
        self.assertEqual(metadata["Name"].value, "foo")
        self.assertEqual(metadata["Version"].value, "1.2.3")
        self.assertRaises(KeyError, metadata.__getitem__, "Homepage")

    def test_from_dict_empty(self):
        d: dict[str, Any] = {}
        metadata = UpstreamMetadata.from_dict(d)
        self.assertRaises(KeyError, metadata.__getitem__, "Name")
        self.assertRaises(KeyError, metadata.__getitem__, "Version")
        self.assertRaises(KeyError, metadata.__getitem__, "Homepage")

    def test_from_dict_invalid(self):
        d = {
            "Name": "foo",
            "Version": "1.2.3",
            "Homepage": 42,
        }
        with self.assertRaises(TypeError):
            UpstreamMetadata.from_dict(d)

    def test_from_dict_yaml(self):
        from ruamel.yaml import YAML

        yaml = YAML()
        d = yaml.load("""Name: foo
Version: 1.2.3  # comment
Homepage: https://example.com
""")
        metadata = UpstreamMetadata.from_dict(d)
        self.assertEqual(metadata["Name"].value, "foo")
        self.assertEqual(metadata["Version"].value, "1.2.3")
        self.assertEqual(metadata["Homepage"].value, "https://example.com")

    def test_from_dict_registry(self):
        d = {
            "Name": "foo",
            "Version": "1.2.3",
            "Registry": [{"Name": "conda:conda-forge", "Entry": "r-tsne"}],
        }
        metadata = UpstreamMetadata.from_dict(d)
        self.assertEqual(
            metadata["Registry"].value,
            [{"Name": "conda:conda-forge", "Entry": "r-tsne"}],
        )
