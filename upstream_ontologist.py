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
- Archive
"""

import os
import re
from urllib.parse import urlparse, urlunparse, parse_qs
from warnings import warn

from debian.deb822 import Deb822

SUPPORTED_CERTAINTIES = ['certain', 'confident', 'likely', 'possible', None]


def certainty_to_confidence(certainty):
    if certainty in ('unknown', None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


def confidence_to_certainty(confidence):
    if confidence is None:
        return 'unknown'
    try:
        return SUPPORTED_CERTAINTIES[confidence] or 'unknown'
    except IndexError:
        raise ValueError(confidence)


def min_certainty(certainties):
    return confidence_to_certainty(
        max([certainty_to_confidence(c)
            for c in certainties] + [0]))


KNOWN_HOSTING_SITES = [
    'code.launchpad.net', 'github.com', 'launchpad.net', 'git.openstack.org']


def plausible_vcs_url(url: str) -> bool:
    return ':' in url


def plausible_vcs_browse_url(url: str) -> bool:
    return url.startswith('https://') or url.startswith('http://')


KNOWN_GITLAB_SITES = [
    'gitlab.com',
    'salsa.debian.org',
    'gitlab.gnome.org',
    'gitlab.freedesktop.org',
    'gitlab.labs.nic.cz',
    'invent.kde.org',
    ]


def is_gitlab_site(hostname: str) -> bool:
    if hostname is None:
        return False
    if hostname in KNOWN_GITLAB_SITES:
        return True
    if hostname.startswith('gitlab.'):
        return True
    return False


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


def guess_repo_from_url(url, net_access=False):
    parsed_url = urlparse(url)
    path_elements = parsed_url.path.strip('/').split('/')
    if parsed_url.netloc == 'github.com':
        if len(path_elements) < 2:
            return None
        return ('https://github.com' +
                '/'.join(parsed_url.path.split('/')[:3]))
    if parsed_url.netloc == 'launchpad.net':
        return 'https://code.launchpad.net/%s' % (
            parsed_url.path.strip('/').split('/')[0])
    if parsed_url.netloc == 'git.savannah.gnu.org':
        if len(path_elements) != 2 or path_elements[0] != 'git':
            return None
        return url
    if parsed_url.netloc in ('freedesktop.org', 'www.freedesktop.org'):
        if len(path_elements) >= 2 and path_elements[0] == 'software':
            return 'https://github.com/freedesktop/%s' % path_elements[1]
        if len(path_elements) >= 3 and path_elements[:2] == [
                'wiki', 'Software']:
            return 'https://github.com/freedesktop/%s.git' % path_elements[2]
    if parsed_url.netloc == 'download.gnome.org':
        if len(path_elements) >= 2 and path_elements[0] == 'sources':
            return 'https://gitlab.gnome.org/GNOME/%s.git' % path_elements[1]
    if parsed_url.netloc == 'download.kde.org':
        if len(path_elements) >= 2 and path_elements[0] in (
                'stable', 'unstable'):
            return 'https://anongit.kde.org/%s.git' % path_elements[1]
    if parsed_url.netloc == 'ftp.gnome.org':
        if (len(path_elements) >= 4 and [
              e.lower() for e in path_elements[:3]] == [
                  'pub', 'gnome', 'sources']):
            return 'https://gitlab.gnome.org/GNOME/%s.git' % path_elements[3]
    if parsed_url.netloc == 'sourceforge.net':
        if (len(path_elements) >= 4 and path_elements[0] == 'p'
                and path_elements[3] == 'ci'):
            return 'https://sourceforge.net/p/%s/%s' % (
                path_elements[1], path_elements[2])
    if parsed_url.netloc == 'www.apache.org':
        if len(path_elements) > 2 and path_elements[0] == 'dist':
            return 'https://svn.apache.org/repos/asf/%s/%s' % (
                path_elements[1], path_elements[2])
    if parsed_url.netloc == 'bitbucket.org':
        if len(path_elements) >= 2:
            return 'https://bitbucket.org/%s/%s' % (
                path_elements[0], path_elements[1])
    if parsed_url.netloc == 'ftp.gnu.org':
        if len(path_elements) >= 2 and path_elements[0] == 'gnu':
            return 'https://git.savannah.gnu.org/git/%s.git' % (
                path_elements[1])
        return None
    if parsed_url.netloc == 'download.savannah.gnu.org':
        if len(path_elements) >= 2 and path_elements[0] == 'releases':
            return 'https://git.savannah.gnu.org/git/%s.git' % (
                path_elements[1])
        return None
    if is_gitlab_site(parsed_url.netloc):
        if parsed_url.path.strip('/').count('/') < 1:
            return None
        parts = parsed_url.path.split('/')
        if 'issues' in parts:
            parts = parts[:parts.index('issues')]
        if 'tags' in parts:
            parts = parts[:parts.index('tags')]
        if parts[-1] == '-':
            parts.pop(-1)
        return urlunparse(
            parsed_url._replace(path='/'.join(parts), query=''))
    if parsed_url.hostname == 'git.php.net':
        if parsed_url.path.startswith('/repository/'):
            return url
        if not parsed_url.path.strip('/'):
            qs = parse_qs(parsed_url.query)
            if 'p' in qs:
                return urlunparse(parsed_url._replace(
                    path='/repository/' + qs['p'][0], query=''))
    if parsed_url.netloc in KNOWN_HOSTING_SITES:
        return url
    # Maybe it's already pointing at a VCS repo?
    if parsed_url.netloc.startswith('svn.'):
        # 'svn' subdomains are often used for hosting SVN repositories.
        return url
    return None


def known_bad_guess(datum):
    if datum.field in ('Bug-Submit', 'Bug-Database'):
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'bugzilla.gnome.org':
            return True
        if parsed_url.hostname == 'bugs.freedesktop.org':
            return True
    if datum.field == 'Repository':
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'anongit.kde.org':
            return True
        if parsed_url.hostname == 'git.gitorious.org':
            return True
    if datum.field == 'Repository-Browse':
        parsed_url = urlparse(datum.value)
        if parsed_url.hostname == 'cgit.kde.org':
            return True
    if datum.value.lower() == 'unknown':
        return True
    return False


def filter_bad_guesses(guessed_items):
    return filter(lambda x: not known_bad_guess(x), guessed_items)


def update_from_guesses(upstream_metadata, guessed_items):
    changed = False
    for datum in guessed_items:
        current_datum = upstream_metadata.get(datum.field)
        if not current_datum or (
                certainty_to_confidence(datum.certainty) <
                certainty_to_confidence(current_datum.certainty)):
            upstream_metadata[datum.field] = datum
            changed = True
    return changed


def guess_from_debian_watch(path, trust_package):
    from debmutate.watch import (
        parse_watch_file,
        MissingVersion,
        )

    def get_package_name():
        with open(os.path.join(os.path.dirname(path), 'control'), 'r') as f:
            return Deb822(f)['Source']
    with open(path, 'r') as f:
        try:
            wf = parse_watch_file(f)
        except MissingVersion:
            return
        if not wf:
            return
        for w in wf:
            url = w.format_url(package=get_package_name)
            if 'mode=git' in w.options:
                yield UpstreamDatum(
                    "Repository", url, "confident",
                    origin=path)
                continue
            if url.startswith('https://') or url.startswith('http://'):
                repo = guess_repo_from_url(url)
                if repo:
                    yield UpstreamDatum(
                        "Repository", repo, "likely",
                        origin=path)
                    continue
            m = re.match('https?://sf.net/([^/]+)', url)
            if m:
                yield UpstreamDatum(
                    "Archive", "SourceForge", "certain",
                    origin=path)
                yield UpstreamDatum(
                    "X-SourceForge-Project", m.group(1), "certain",
                    origin=path)
                continue


def guess_from_debian_control(path, trust_package):
    with open(path, 'r') as f:
        control = Deb822(f)
    if 'Homepage' in control:
        yield UpstreamDatum('Homepage', control['Homepage'], 'certain')
    if 'XS-Go-Import-Path' in control:
        yield (
            UpstreamDatum(
                'Repository',
                'https://' + control['XS-Go-Import-Path'],
                'likely'))


def guess_from_python_metadata(pkg_info):
    if 'Name' in pkg_info:
        yield UpstreamDatum('Name', pkg_info['name'], 'certain')
    if 'Version' in pkg_info:
        yield UpstreamDatum('X-Version', pkg_info['Version'], 'certain')
    if 'Home-Page' in pkg_info:
        repo = guess_repo_from_url(pkg_info['Home-Page'])
        if repo:
            yield UpstreamDatum(
                'Repository', repo, 'likely')
    for value in pkg_info.get_all('Project-URL', []):
        url_type, url = value.split(', ')
        if url_type in ('GitHub', 'Repository', 'Source Code'):
            yield UpstreamDatum(
                'Repository', url, 'certain')
        if url_type in ('Bug Tracker', ):
            yield UpstreamDatum(
                'Bug-Database', url, 'certain')
    if 'Summary' in pkg_info:
        yield UpstreamDatum('X-Summary', pkg_info['Summary'], 'certain')
    payload = pkg_info.get_payload()
    if payload.strip() and pkg_info.get_content_type() in (None, 'text/plain'):
        yield UpstreamDatum(
            'X-Description', pkg_info.get_payload(), 'possible')


def guess_from_pkg_info(path, trust_package):
    """Get the metadata from a python setup.py file."""
    from email.parser import Parser
    try:
        with open(path, 'r') as f:
            pkg_info = Parser().parse(f)
    except FileNotFoundError:
        return
    yield from guess_from_python_metadata(pkg_info)


def xmlparse_simplify_namespaces(path, namespaces):
    import xml.etree.ElementTree as ET
    namespaces = ['{%s}' % ns for ns in namespaces]
    tree = ET.iterparse(path)
    for _, el in tree:
        for namespace in namespaces:
            el.tag = el.tag.replace(namespace, '')
    return tree.root


def guess_from_dist_ini(path, trust_package):
    from configparser import (
        RawConfigParser,
        NoSectionError,
        ParsingError,
        NoOptionError,
        )
    parser = RawConfigParser(strict=False)
    with open(path, 'r') as f:
        try:
            parser.read_string('[START]\n' + f.read())
        except ParsingError as e:
            warn('Unable to parse dist.ini: %r' % e)
    try:
        yield UpstreamDatum('Name', parser['START']['name'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield UpstreamDatum(
            'Bug-Database', parser['MetaResources']['bugtracker.web'],
            'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass
    try:
        yield UpstreamDatum(
            'Repository', parser['MetaResources']['repository.url'], 'certain')
    except (NoSectionError, NoOptionError, KeyError):
        pass


def guess_from_debian_copyright(path, trust_package):
    from debian.copyright import (
        Copyright,
        NotMachineReadableError,
        MachineReadableFormatError,
        )
    with open(path, 'r') as f:
        try:
            copyright = Copyright(f, strict=False)
        except NotMachineReadableError:
            header = None
        except MachineReadableFormatError as e:
            warn('Error parsing copyright file: %s' % e)
            header = None
        except ValueError as e:
            # This can happen with an error message of
            # ValueError: value must not have blank lines
            warn('Error parsing copyright file: %s' % e)
            header = None
        else:
            header = copyright.header
    if header:
        if header.upstream_name:
            yield UpstreamDatum("Name", header.upstream_name, 'certain')
        if header.upstream_contact:
            yield UpstreamDatum(
                "Contact", ','.join(header.upstream_contact), 'certain')
        if header.source:
            if ' ' in header.source:
                from_urls = [u for u in re.split('[ ,\n]', header.source) if u]
            else:
                from_urls = [header.source]
            for from_url in from_urls:
                repo_url = guess_repo_from_url(from_url)
                if repo_url:
                    yield UpstreamDatum(
                        'Repository', repo_url, 'likely')
                if (from_url.startswith('https://pecl.php.net/package/') or
                        from_url.startswith('http://pecl.php.net/package/')):
                    yield UpstreamDatum('X-Pecl-URL', from_url, 'certain')
        if "X-Upstream-Bugs" in header:
            yield UpstreamDatum(
                "Bug-Database", header["X-Upstream-Bugs"], 'certain')
        if "X-Source-Downloaded-From" in header:
            url = guess_repo_from_url(header["X-Source-Downloaded-From"])
            if url is not None:
                yield UpstreamDatum("Repository", url, 'certain')


def guess_from_debian_patch(path, trust_package):
    with open(path, 'rb') as f:
        for line in f:
            if line.startswith(b'Forwarded: '):
                forwarded = line.split(b':', 1)[1].strip()
                bug_db = bug_database_from_issue_url(forwarded.decode('utf-8'))
                if bug_db:
                    yield UpstreamDatum('Bug-Database', bug_db, 'possible')
                repo_url = repo_url_from_merge_request_url(
                    forwarded.decode('utf-8'))
                if repo_url:
                    yield UpstreamDatum('Repository', repo_url, 'possible')


def guess_from_meta_json(path, trust_package):
    import json
    with open(path, 'r') as f:
        data = json.load(f)
        if 'name' in data:
            yield UpstreamDatum('Name', data['name'], 'certain')
        if 'resources' in data:
            resources = data['resources']
            if 'bugtracker' in resources and 'web' in resources['bugtracker']:
                yield UpstreamDatum(
                    "Bug-Database", resources["bugtracker"]["web"], 'certain')
                # TODO(jelmer): Support resources["bugtracker"]["mailto"]
            if 'homepage' in resources:
                yield UpstreamDatum(
                    "Homepage", resources["homepage"], 'certain')
            if 'repository' in resources:
                repo = resources['repository']
                if 'url' in repo:
                    yield UpstreamDatum(
                        'Repository', repo["url"], 'certain')
                if 'web' in repo:
                    yield UpstreamDatum(
                        'Repository-Browse', repo['web'], 'certain')


def guess_from_doap(path, trust_package):
    """Guess upstream metadata from a DOAP file.
    """
    from xml.etree import ElementTree
    el = ElementTree.parse(path)
    root = el.getroot()
    DOAP_NAMESPACE = 'http://usefulinc.com/ns/doap#'
    if root.tag == '{http://www.w3.org/1999/02/22-rdf-syntax-ns#}RDF':
        # If things are wrapped in RDF, unpack.
        [root] = list(root)

    if root.tag != ('{%s}Project' % DOAP_NAMESPACE):
        warn('Doap file does not have DOAP project as root')
        return

    def extract_url(el):
        return el.attrib.get(
            '{http://www.w3.org/1999/02/22-rdf-syntax-ns#}resource')

    for child in root:
        if child.tag == ('{%s}name' % DOAP_NAMESPACE) and child.text:
            yield UpstreamDatum('Name', child.text, 'certain')
        if child.tag == ('{%s}bug-database' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield UpstreamDatum('Bug-Database', url, 'certain')
        if child.tag == ('{%s}homepage' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield UpstreamDatum('Homepage', url, 'certain')
        if child.tag == ('{%s}download-page' % DOAP_NAMESPACE):
            url = extract_url(child)
            if url:
                yield UpstreamDatum('X-Download', url, 'certain')
        if child.tag == ('{%s}repository' % DOAP_NAMESPACE):
            for repo in child:
                if repo.tag in (
                        '{%s}SVNRepository' % DOAP_NAMESPACE,
                        '{%s}GitRepository' % DOAP_NAMESPACE):
                    repo_location = repo.find(
                        '{http://usefulinc.com/ns/doap#}location')
                    if repo_location is not None:
                        repo_url = extract_url(repo_location)
                    else:
                        repo_url = None
                    if repo_url:
                        yield UpstreamDatum(
                            'Repository', repo_url,
                            'certain')
                    web_location = repo.find(
                        '{http://usefulinc.com/ns/doap#}browse')
                    if web_location is not None:
                        web_url = extract_url(web_location)
                    else:
                        web_url = None

                    if web_url:
                        yield UpstreamDatum(
                            'Repository-Browse', web_url, 'certain')


def is_email_address(value: str) -> bool:
    return '@' in value or ' (at) ' in value


def guess_from_configure(path, trust_package=False):
    if os.path.isdir(path):
        return
    with open(path, 'rb') as f:
        for line in f:
            if b'=' not in line:
                continue
            (key, value) = line.strip().split(b'=', 1)
            if b' ' in key:
                continue
            if b'$' in value:
                continue
            value = value.strip()
            if value.startswith(b"'") and value.endswith(b"'"):
                value = value[1:-1]
            if not value:
                continue
            if key == b'PACKAGE_NAME':
                yield UpstreamDatum(
                    'Name', value.decode(), 'certain', './configure')
            elif key == b'PACKAGE_VERSION':
                yield UpstreamDatum(
                    'X-Version', value.decode(), 'certain', './configure')
            elif key == b'PACKAGE_BUGREPORT':
                if value in (b'BUG-REPORT-ADDRESS', ):
                    certainty = 'invalid'
                elif (is_email_address(value.decode()) and
                        not value.endswith(b'gnu.org')):
                    # Downgrade the trustworthiness of this field for most
                    # upstreams if it contains an e-mail address. Most
                    # upstreams seem to just set this to some random address,
                    # and then forget about it.
                    certainty = 'possible'
                else:
                    parsed_url = urlparse(value.decode())
                    if parsed_url.path.strip('/'):
                        certainty = 'certain'
                    else:
                        # It seems unlikely that the bug submit URL lives at
                        # the root.
                        certainty = 'possible'
                if certainty != 'invalid':
                    yield UpstreamDatum(
                        'Bug-Submit', value.decode(), certainty, './configure')
            elif key == b'PACKAGE_URL':
                yield UpstreamDatum(
                    'Homepage', value.decode(), 'certain', './configure')


def guess_from_cargo(path, trust_package):
    try:
        from toml.decoder import load, TomlDecodeError
    except ImportError:
        return
    try:
        with open(path, 'r') as f:
            cargo = load(f)
    except FileNotFoundError:
        return
    except TomlDecodeError as e:
        warn('Error parsing toml file %s: %s' % (path, e))
        return
    try:
        package = cargo['package']
    except KeyError:
        pass
    else:
        if 'name' in package:
            yield UpstreamDatum('Name', package['name'], 'certain')
        if 'description' in package:
            yield UpstreamDatum('X-Summary', package['description'], 'certain')
        if 'homepage' in package:
            yield UpstreamDatum('Homepage', package['homepage'], 'certain')
        if 'license' in package:
            yield UpstreamDatum('X-License', package['license'], 'certain')
        if 'repository' in package:
            yield UpstreamDatum('Repository', package['repository'], 'certain')
        if 'version' in package:
            yield UpstreamDatum('X-Version', package['version'], 'confident')


def guess_from_pom_xml(path, trust_package=False):
    # Documentation: https://maven.apache.org/pom.html

    import xml.etree.ElementTree as ET
    try:
        root = xmlparse_simplify_namespaces(path, [
            'http://maven.apache.org/POM/4.0.0'])
    except ET.ParseError as e:
        warn('Unable to parse package.xml: %s' % e)
        return
    assert root.tag == 'project', 'root tag is %r' % root.tag
    name_tag = root.find('name')
    if name_tag is not None:
        yield UpstreamDatum('Name', name_tag.text, 'certain')
    description_tag = root.find('description')
    if description_tag is not None:
        yield UpstreamDatum('X-Summary', description_tag.text, 'certain')
    version_tag = root.find('version')
    if version_tag is not None and '$' not in version_tag.text:
        yield UpstreamDatum('X-Version', version_tag.text, 'certain')
    licenses_tag = root.find('licenses')
    if licenses_tag is not None:
        licenses = []
        for license_tag in licenses_tag.findall('license'):
            name_tag = license_tag.find('name')
            if name_tag is not None:
                licenses.append(name_tag.text)
    for scm_tag in root.findall('scm'):
        url_tag = scm_tag.find('url')
        if url_tag is not None:
            if (url_tag.text.startswith('scm:') and
                    url_tag.text.count(':') >= 3):
                url = url_tag.text.split(':', 2)[2]
            else:
                url = url_tag.text
            if plausible_vcs_browse_url(url):
                yield UpstreamDatum('Repository-Browse', url, 'certain')
        connection_tag = scm_tag.find('connection')
        if connection_tag is not None:
            connection = connection_tag.text
            try:
                (scm, provider, provider_specific) = connection.split(':', 2)
            except ValueError:
                warn('Invalid format for SCM connection: %s' % connection)
                continue
            if scm != 'scm':
                warn('SCM connection does not start with scm: prefix: %s' %
                     connection)
                continue
            yield UpstreamDatum(
                'Repository', provider_specific, 'certain')
    for issue_mgmt_tag in root.findall('issueManagement'):
        url_tag = issue_mgmt_tag.find('url')
        if url_tag is not None:
            yield UpstreamDatum('Bug-Database', url_tag.text, 'certain')
    url_tag = root.find('url')
    if url_tag:
        if not url_tag.text.startswith('scm:'):
            # Yeah, uh, not a URL.
            pass
        else:
            yield UpstreamDatum('Homepage', url_tag.text, 'certain')


def _get_guessers(path, trust_package=False):
    CANDIDATES = [
        ('debian/watch', guess_from_debian_watch),
        ('debian/control', guess_from_debian_control),
        ('debian/copyright', guess_from_debian_copyright),
        ('PKG-INFO', guess_from_pkg_info),
        ('dist.ini', guess_from_dist_ini),
        ('META.json', guess_from_meta_json),
        ('configure', guess_from_configure),
        ('Cargo.toml', guess_from_cargo),
        ('pom.xml', guess_from_pom_xml),
        ]

    doap_filenames = [
        n for n in os.listdir(path)
        if n.endswith('.doap')]
    if doap_filenames:
        if len(doap_filenames) == 1:
            CANDIDATES.append((doap_filenames[0], guess_from_doap))
        else:
            warn('More than one doap filename, ignoring all: %r' %
                 doap_filenames)

    try:
        debian_patches = [
            os.path.join('debian', 'patches', n)
            for n in os.listdir('debian/patches')
            if os.path.isfile(os.path.join('debian/patches', n))]
    except FileNotFoundError:
        pass
    else:
        CANDIDATES.extend(
            [(p, guess_from_debian_patch) for p in debian_patches])

    for relpath, guesser in CANDIDATES:
        abspath = os.path.join(path, relpath)
        if not os.path.exists(abspath):
            continue
        yield relpath, guesser(abspath, trust_package=trust_package)


def guess_upstream_metadata_items(path: str, trust_package: bool = False):
    """Guess upstream metadata items, in no particular order.

    Args:
      path: Path to the package
      trust: Whether to trust the package contents and i.e. run
      executables in it
    Yields:
      UpstreamDatum
    """
    for entry in guess_upstream_info(path, trust_package=trust_package):
        if isinstance(entry, UpstreamDatum):
            yield entry


def guess_upstream_info(
        path: str, trust_package: bool = False):
    guessers = _get_guessers(path, trust_package=trust_package)
    for name, guesser in guessers:
        for entry in guesser:
            if entry.origin is None:
                entry.origin = name
            yield entry


def summarize_upstream_metadata(
        metadata_items, path, net_access=False):
    """Summarize the upstream metadata into a dictionary.

    Args:
      metadata_items: Iterator over metadata items
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
      net_access: Whether to allow net access
    """
    upstream_metadata = {}
    update_from_guesses(
        upstream_metadata,
        filter_bad_guesses(metadata_items))

    return {k: v.value for (k, v) in upstream_metadata.items()}


def guess_upstream_metadata(
        path, trust_package=False, net_access=False):
    """Guess the upstream metadata dictionary.

    Args:
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
      net_access: Whether to allow net access
    """
    metadata_items = guess_upstream_metadata_items(
        path, trust_package=trust_package)
    return summarize_upstream_metadata(
        metadata_items, path, net_access=net_access)


def repo_url_from_merge_request_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) > 2 and path_elements[2] == 'issues':
            return urlunparse(
                ('https', 'github.com', '/'.join(path_elements[:3]),
                 None, None, None))
    if is_gitlab_site(parsed_url.netloc):
        path_elements = parsed_url.path.strip('/').split('/')
        if (len(path_elements) > 2 and
                path_elements[-2] == 'merge_requests' and
                path_elements[-1].isdigit()):
            return urlunparse(
                ('https', parsed_url.netloc, '/'.join(path_elements[:-2]),
                 None, None, None))


def bug_database_from_issue_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path_elements = parsed_url.path.strip('/').split('/')
        if len(path_elements) > 2 and path_elements[2] == 'issues':
            return urlunparse(
                ('https', 'github.com', '/'.join(path_elements[:3]),
                 None, None, None))
    if is_gitlab_site(parsed_url.netloc):
        path_elements = parsed_url.path.strip('/').split('/')
        if (len(path_elements) > 2 and
                path_elements[-2] == 'issues' and
                path_elements[-1].isdigit()):
            return urlunparse(
                ('https', parsed_url.netloc, '/'.join(path_elements[:-2]),
                 None, None, None))


def bug_database_url_from_bug_submit_url(url):
    parsed_url = urlparse(url)
    path_elements = parsed_url.path.strip('/').split('/')
    if parsed_url.netloc == 'github.com':
        if len(path_elements) not in (3, 4):
            return None
        if path_elements[2] != 'issues':
            return None
        return urlunparse(
            ('https', 'github.com', '/'.join(path_elements[:3]),
             None, None, None))
    if parsed_url.netloc == 'bugs.launchpad.net':
        if len(path_elements) >= 1:
            return urlunparse(
                parsed_url._replace(path='/%s' % path_elements[0]))
    if is_gitlab_site(parsed_url.netloc):
        if len(path_elements) < 2:
            return None
        if path_elements[-2] != 'issues':
            return None
        if path_elements[-1] == 'new':
            path_elements.pop(-1)
        return urlunparse(
            parsed_url._replace(path='/'.join(path_elements)))
    if parsed_url.hostname == 'sourceforge.net':
        if len(path_elements) < 3:
            return None
        if path_elements[0] != 'p' or path_elements[2] != 'bugs':
            return None
        if len(path_elements) > 3:
            path_elements.pop(-1)
        return urlunparse(
            parsed_url._replace(path='/'.join(path_elements)))
    return None


def bug_submit_url_from_bug_database_url(url):
    parsed_url = urlparse(url)
    path_elements = parsed_url.path.strip('/').split('/')
    if parsed_url.netloc == 'github.com':
        if len(path_elements) != 3:
            return None
        if path_elements[2] != 'issues':
            return None
        return urlunparse(
            ('https', 'github.com', parsed_url.path + '/new',
             None, None, None))
    if parsed_url.netloc == 'bugs.launchpad.net':
        if len(path_elements) == 1:
            return urlunparse(
                parsed_url._replace(path=parsed_url.path+'/+filebug'))
    if is_gitlab_site(parsed_url.netloc):
        if len(path_elements) < 2:
            return None
        if path_elements[-1] != 'issues':
            return None
        return urlunparse(
            parsed_url._replace(path=parsed_url.path.rstrip('/')+'/new'))
    return None


def _extrapolate_repository_from_homepage(upstream_metadata, net_access):
    repo = guess_repo_from_url(
            upstream_metadata['Homepage'].value, net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            min_certainty(['likely', upstream_metadata['Homepage'].certainty]))


def _extrapolate_repository_from_download(upstream_metadata, net_access):
    repo = guess_repo_from_url(
            upstream_metadata['X-Download'].value, net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            min_certainty(
                ['likely', upstream_metadata['X-Download'].certainty]))


def _extrapolate_repository_from_bug_db(upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Bug-Database'].value, net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            min_certainty(
                ['likely', upstream_metadata['Bug-Database'].certainty]))


def _extrapolate_name_from_repository(upstream_metadata, net_access):
    repo = guess_repo_from_url(
            upstream_metadata['Repository'].value, net_access=net_access)
    if repo:
        parsed = urlparse(repo)
        name = parsed.path.split('/')[-1]
        if name.endswith('.git'):
            name = name[:-4]
        return UpstreamDatum('Name', name, min_certainty(
                ['likely', upstream_metadata['Repository'].certainty]))


def _extrapolate_repository_from_repository_browse(
        upstream_metadata, net_access):
    repo = guess_repo_from_url(
        upstream_metadata['Repository-Browse'].value,
        net_access=net_access)
    if repo:
        return UpstreamDatum(
            'Repository', repo,
            upstream_metadata['Repository-Browse'].certainty)


def _extrapolate_bug_submit_from_bug_db(
        upstream_metadata, net_access):
    bug_submit_url = bug_submit_url_from_bug_database_url(
        upstream_metadata['Bug-Database'].value)
    if bug_submit_url:
        return UpstreamDatum(
            'Bug-Submit', bug_submit_url,
            upstream_metadata['Bug-Database'].certainty)


def _extrapolate_bug_db_from_bug_submit(
        upstream_metadata, net_access):
    bug_db_url = bug_database_url_from_bug_submit_url(
        upstream_metadata['Bug-Submit'].value)
    if bug_db_url:
        return UpstreamDatum(
            'Bug-Database', bug_db_url,
            upstream_metadata['Bug-Submit'].certainty)


def _copy_bug_db_field(upstream_metadata, net_access):
    ret = UpstreamDatum(
        'Bug-Database',
        upstream_metadata['Bugs-Database'].value,
        upstream_metadata['Bugs-Database'].certainty,
        upstream_metadata['Bugs-Database'].origin)
    del upstream_metadata['Bugs-Database']
    return ret


EXTRAPOLATE_FNS = [
    (['Homepage'], 'Repository', _extrapolate_repository_from_homepage),
    (['Bugs-Database'], 'Bug-Database', _copy_bug_db_field),
    (['Bug-Database'], 'Repository', _extrapolate_repository_from_bug_db),
    (['Repository-Browse'], 'Repository',
     _extrapolate_repository_from_repository_browse),
    (['Bug-Database'], 'Bug-Submit', _extrapolate_bug_submit_from_bug_db),
    (['Bug-Submit'], 'Bug-Database', _extrapolate_bug_db_from_bug_submit),
    (['X-Download'], 'Repository', _extrapolate_repository_from_download),
    (['Repository'], 'Name', _extrapolate_name_from_repository),
]


def _version_in_tags(version, tag_names):
    if version in tag_names:
        return True
    if 'v%s' % version in tag_names:
        return True
    if 'release/%s' % version in tag_names:
        return True
    if version.replace('.', '_') in tag_names:
        return True
    for tag_name in tag_names:
        if tag_name.endswith('_' + version):
            return True
        if tag_name.endswith('-' + version):
            return True
        if tag_name.endswith('_%s' % version.replace('.', '_')):
            return True
    return False


if __name__ == '__main__':
    import argparse
    import sys
    import ruamel.yaml
    parser = argparse.ArgumentParser(sys.argv[0])
    parser.add_argument('path', default='.', nargs='?')
    parser.add_argument(
        '--trust',
        action='store_true',
        help='Whether to allow running code from the package.')
    parser.add_argument(
        '--disable-net-access',
        help='Do not probe external services.',
        action='store_true', default=False)
    parser.add_argument(
        '--scan', action='store_true',
        help='Scan for metadata rather than printing results.')
    args = parser.parse_args(sys.argv[1:])

    if args.scan:
        for entry in guess_upstream_info(args.path, args.trust):
            print('%s: %r - certainty %s (from %s)' % (
                  entry.field, entry.value, entry.certainty, entry.origin))
    else:
        metadata = guess_upstream_metadata(
            args.path, args.trust, not args.disable_net_access)

        sys.stdout.write(ruamel.yaml.round_trip_dump(metadata))
