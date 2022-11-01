#!/usr/bin/python3
# Copyright (C) 2022 Jelmer Vernooij <jelmer@debian.org>
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

def codemeta_file_from_upstream_info(upstream_info):
    ret = {
        "@context": "https://doi.org/10.5063/schema/codemeta-2.0",
        "@type": "SoftwareSourceCode",
    }
    if "Name" in upstream_info:
        ret["name"] = upstream_info["Name"]
    if "X-Version" in upstream_info:
        ret["version"] = upstream_info["X-Version"]
    if "Repository" in upstream_info:
        ret["codeRepository"] = upstream_info["Repository"]
    if "Bug-Database" in upstream_info:
        ret["issueTracker"] = upstream_info["Bug-Database"]
    # TODO(jelmer): Support setting contIntegration
    if "X-License" in upstream_info:
        ret["license"] = "https://spdx.org/licenses/%s" % upstream_info["X-License"]
    if "X-Description" in upstream_info:
        ret["description"] = upstream_info["X-Description"]
    # TODO(jelmer): Support keywords
    # TODO(jelmer): Support funder
    # TODO(jelmer): Support funding
    # TODO(jelmer): Support creation date
    # TODO(jelmer): Support first release date
    # TODO(jelmer): Support unique identifier
    # TODO(jelmer): Support runtime platform
    # TODO(jelmer): Support other software requirements
    # TODO(jelmer): Support operating system
    # TODO(jelmer): Support development status
    # TODO(jelmer): Support reference publication
    # TODO(jelmer): Support part of
    # TODO(jelmer): Support Author
    for link_field in ["Documentation", "Homepage"]:
        if link_field in upstream_info:
            ret.setdefault("relatedLink", []).append(upstream_info[link_field])
    if "X-Download" in upstream_info:
        ret["downloadUrl"] = upstream_info["X-Download"]
    return ret


def main(argv=None):
    from .guess import get_upstream_info
    import argparse
    import sys
    import json

    if argv is None:
        argv = sys.argv

    parser = argparse.ArgumentParser(argv)
    parser.add_argument("path", default=".", nargs="?")

    parser.add_argument(
        "--trust",
        action="store_true",
        help="Whether to allow running code from the package.",
    )
    parser.add_argument(
        "--disable-net-access",
        help="Do not probe external services.",
        action="store_true",
        default=False,
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Check guessed metadata against external sources.",
    )

    args = parser.parse_args()

    upstream_info = get_upstream_info(
        args.path, trust_package=args.trust,
        net_access=not args.disable_net_access,
        check=args.check)

    codemeta = codemeta_file_from_upstream_info(upstream_info)

    json.dump(codemeta, sys.stdout, indent=4)


if __name__ == '__main__':
    import sys
    sys.exit(main(sys.argv))
