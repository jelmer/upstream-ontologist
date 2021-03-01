Upstream Ontologist
===================

The upstream ontologist provides a common interface for finding metadata about
upstream software projects.

It will gather information from any sources available, prioritize data that it
has higher confidence in as well as report the confidence for each of the
bits of metadata.

The ontologist originated in Debian and the currently reported metadata fields
are loosely based on [DEP-12](https://dep-team.pages.debian.net/deps/dep12),
but it is meant to be distribution-agnostic.

Provided Fields
---------------

Standard fields:

 * Homepage: homepage URL
 * Name: human name of the upstream project
 * Contact: contact address of some sort of the upstream (e-mail, mailing list URL)
 * Repository: VCS URL
 * Repository-Browse: Web URL for viewing the VCS
 * Bug-Database: Bug database URL (for web viewing, generally)
 * Bug-Submit: URL to use to submit new bugs (either on the web or an e-mail address)
 * Screenshots: List of URLs with screenshots
 * Archive: Archive used - e.g. SourceForge
 * Security-Contact: e-mail or URL with instructions for reporting security issues

Extensions for upstream-ontologist, not defined in DEP-12:

 * X-SourceForge-Project: sourceforge project name
 * X-Wiki: Wiki URL
 * X-Summary: one-line description of the project
 * X-Description: longer description of the project
 * X-License: Single line license (e.g. "GPL 2.0")
 * X-Copyright: List of copyright holders
 * X-Version: Current upstream version
 * X-Security-MD: URL to markdown file with security policy

Supported Data Sources
----------------------

At the moment, the ontologist can read metadata from the following upstream
data sources:

 * Python package metadata (PKG-INFO, setup.py)
 * package.json
 * dist.ini
 * package.xml
 * dist.ini
 * META.json
 * META.yml
 * GNU configure files
 * R DESCRIPTION files
 * Rust Cargo.toml
 * Maven pom.xml
 * .git/config
 * SECURITY.md
 * DOAP
 * Haskell cabal files
 * Debian packaging metadata
   (debian/watch, debian/control, debian/rules, debian/get-orig-source.sh,
    debian/copyright, debian/patches)

It will also scan README for possible upstream repository URLs
(and will attempt to verify that those match the local repository).

In addition to local files, it can also consult external directories
using their APIs:

 * GitHub
 * SourceForge
 * repology
 * Launchpad
 * PECL
 * AUR

Example Usage
-------------

The easiest way to use the upstream ontologist is by invoking the
``guess-upstream-metadata`` command in a software project:

```console
$ guess-upstream-metadata ~/src/dulwich
X-Security-MD: https://github.com/dulwich/dulwich/tree/HEAD/SECURITY.md
Name: dulwich
X-Version: 0.20.15
Bug-Database: https://github.com/dulwich/dulwich/issues
Repository: https://www.dulwich.io/code/
X-Summary: Python Git Library
Bug-Submit: https://github.com/dulwich/dulwich/issues/new
```

Alternatively, there is a Python API.
