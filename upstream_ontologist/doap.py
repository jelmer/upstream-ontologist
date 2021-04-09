#!/usr/bin/python3
# Copyright (C) 2021 Jelmer Vernooij <jelmer@debian.org>
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

from lxml import etree

Element = etree.Element
SubElement = etree.SubElement
tostring = etree.tostring

RDF_NS = "http://www.w3.org/1999/02/22-rdf-syntax-ns"
etree.register_namespace('rdf', RDF_NS)
FOAF_NS = "http://xmlns.com/foaf/0.1/"
etree.register_namespace('foaf', FOAF_NS)
DOAP_NS = "http://usefulinc.com/ns/doap"
etree.register_namespace('doap', DOAP_NS)


def doap_file_from_upstream_info(upstream_info):
    project = Element('{%s}Project' % DOAP_NS)

    if 'Name' in upstream_info:
        SubElement(project, '{%s}name' % DOAP_NS).text = upstream_info['Name']

    if 'Homepage' in upstream_info:
        hp = SubElement(project, '{%s}homepage' % DOAP_NS)
        hp.set('{%s}resource' % RDF_NS, upstream_info['Homepage'])

    if 'X-Summary' in upstream_info:
        sd = SubElement(project, '{%s}shortdesc' % DOAP_NS)
        sd.text = upstream_info['X-Summary']

    if 'X-Description' in upstream_info:
        sd = SubElement(project, '{%s}description' % DOAP_NS)
        sd.text = upstream_info['X-Description']

    if 'X-Download' in upstream_info:
        dp = SubElement(project, '{%s}download-page' % DOAP_NS)
        dp.set('{%s}resource' % RDF_NS, upstream_info['X-Download'])

    if 'Repository' in upstream_info or 'Repository-Browse' in upstream_info:
        repository = SubElement(project, '{%s}repository' % DOAP_NS)
        # TODO(jelmer): how do we know the repository type?
        git_repo = SubElement(repository, '{%s}GitRepository' % DOAP_NS)
        if 'Repository' in upstream_info:
            location = SubElement(git_repo, '{%s}location' % DOAP_NS)
            location.set('{%s}resource' % RDF_NS, upstream_info['Repository'])
        if 'Repository-Browse' in upstream_info:
            location = SubElement(git_repo, '{%s}browse' % DOAP_NS)
            location.set('{%s}resource' % RDF_NS, upstream_info['Repository-Browse'])

    if 'X-Mailing-List' in upstream_info:
        mailinglist = SubElement(project, '{%s}mailing-list' % DOAP_NS)
        mailinglist.set('{%s}resource' % RDF_NS, upstream_info['X-Mailing-List'])

    if 'Bug-Database' in upstream_info:
        bugdb = SubElement(project, '{%s}bug-database' % DOAP_NS)
        bugdb.set('{%s}resource' % RDF_NS, upstream_info['Bug-Database'])

    if 'Screenshots' in upstream_info:
        screenshots = SubElement(project, '{%s}screenshots' % DOAP_NS)
        screenshots.set('{%s}resource' % RDF_NS, upstream_info['Screenshots'])

    if 'Security-Contact' in upstream_info:
        security_contact = SubElement(project, '{%s}security-contact' % DOAP_NS)
        security_contact.set('{%s}resource' % RDF_NS, upstream_info['Security-Contact'])

    if 'X-Wiki' in upstream_info:
        wiki = SubElement(project, '{%s}wiki' % DOAP_NS)
        wiki.set('{%s}resource' % RDF_NS, upstream_info['X-Wiki'])

    return etree.ElementTree(project)


def main(argv=None):
    from .guess import get_upstream_info
    import argparse
    import sys

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

    et = doap_file_from_upstream_info(upstream_info)

    et.write(
        sys.stdout.buffer,
        xml_declaration=True,
        method="xml",
        encoding="utf-8", pretty_print=True)


if __name__ == '__main__':
    import sys
    sys.exit(main(sys.argv))
