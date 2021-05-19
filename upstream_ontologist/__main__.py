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

"""Functions for working with upstream metadata."""

import logging
import os
import sys

from . import (
    version_string,
    UpstreamDatum,
    Person,
)
from .guess import (
    guess_upstream_metadata,
    guess_upstream_info,
)


def main(argv=None):
    import argparse
    import ruamel.yaml

    parser = argparse.ArgumentParser(sys.argv[0])
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
    parser.add_argument(
        "--scan",
        action="store_true",
        help="Scan for metadata rather than printing results.",
    )
    parser.add_argument(
        "--consult-external-directory",
        action="store_true",
        help="Pull in external (not maintained by upstream) directory data",
    )
    parser.add_argument(
        "--version", action="version", version="%(prog)s " + version_string
    )
    parser.add_argument('--verbose', action='store_true')
    args = parser.parse_args(argv)

    if args.verbose:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO)

    if not os.path.isdir(args.path):
        sys.stderr.write("%s is not a directory\n" % args.path)
        return 1

    if args.scan:
        for entry in guess_upstream_info(args.path, args.trust):
            if isinstance(entry, UpstreamDatum):
                print(
                    "%s: %r - certainty %s (from %s)"
                    % (entry.field, entry.value, entry.certainty, entry.origin)
                )
            else:
                raise TypeError(entry)
    else:
        metadata = guess_upstream_metadata(
            args.path,
            args.trust,
            not args.disable_net_access,
            consult_external_directory=args.consult_external_directory,
            check=args.check,
        )

        yaml = ruamel.yaml.YAML()
        ruamel.yaml.scalarstring.walk_tree(metadata)
        yaml.register_class(Person)
        yaml.dump(metadata, sys.stdout)
        return 0


if __name__ == "__main__":
    sys.exit(main())
