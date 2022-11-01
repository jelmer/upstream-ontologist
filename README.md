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

 * ``Homepage``: homepage URL
 * ``Name``: human name of the upstream project
 * ``Contact``: contact address of some sort of the upstream (e-mail, mailing list URL)
 * ``Repository``: VCS URL
 * ``Repository-Browse``: Web URL for viewing the VCS
 * ``Bug-Database``: Bug database URL (for web viewing, generally)
 * ``Bug-Submit``: URL to use to submit new bugs (either on the web or an e-mail address)
 * ``Screenshots``: List of URLs with screenshots
 * ``Archive``: Archive used - e.g. SourceForge
 * ``Security-Contact``: e-mail or URL with instructions for reporting security issues
 * ``Documentation``: Link to documentation on the web

Extensions for upstream-ontologist, not defined in DEP-12:

 * ``X-SourceForge-Project``: sourceforge project name
 * ``X-Wiki``: Wiki URL
 * ``X-Summary``: one-line description of the project
 * ``X-Description``: longer description of the project
 * ``X-License``: Single line license (e.g. "GPL 2.0")
 * ``X-Copyright``: List of copyright holders
 * ``X-Version``: Current upstream version
 * ``X-Security-MD``: URL to markdown file with security policy
 * ``X-Author``: List of people who contributed to the project
 * ``X-Maintainer``: The maintainer of the project

Supported Data Sources
----------------------

At the moment, the ontologist can read metadata from the following upstream
data sources:

 * Python package metadata (PKG-INFO, setup.py, setup.cfg, pyproject.timl)
 * [package.json](https://docs.npmjs.com/cli/v7/configuring-npm/package-json)
 * [composer.json](https://getcomposer.org/doc/04-schema.md)
 * [package.xml](https://pear.php.net/manual/en/guide.developers.package2.dependencies.php)
 * Perl package metadata (dist.ini, META.json, META.yml, Makefile.PL)
 * [Perl POD files](https://perldoc.perl.org/perlpod)
 * GNU configure files
 * [R DESCRIPTION files](https://r-pkgs.org/description.html)
 * [Rust Cargo.toml](https://doc.rust-lang.org/cargo/reference/manifest.html)
 * [Maven pom.xml](https://maven.apache.org/pom.html)
 * [metainfo.xml](https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html)
 * [.git/config](https://git-scm.com/docs/git-config)
 * SECURITY.md
 * [DOAP](https://github.com/ewilderj/doap)
 * [Haskell cabal files](https://cabal.readthedocs.io/en/3.4/cabal-package.html)
 * [go.mod](https://golang.org/doc/modules/gomod-ref)
 * [ruby gemspec files](https://guides.rubygems.org/specification-reference/)
 * [nuspec files](https://docs.microsoft.com/en-us/nuget/reference/nuspec)
 * [OPAM files](https://opam.ocaml.org/doc/Manual.html#Package-definitions)
 * Debian packaging metadata
   (debian/watch, debian/control, debian/rules, debian/get-orig-source.sh,
    debian/copyright, debian/patches)
 * Dart's [pubspec.yaml](https://dart.dev/tools/pub/pubspec)
 * meson.build

It will also scan README and INSTALL for possible upstream repository URLs
(and will attempt to verify that those match the local repository).

In addition to local files, it can also consult external directories
using their APIs:

 * [GitHub](https://github.com/)
 * [SourceForge](https://sourceforge.net/)
 * [repology](https://www.repology.org/)
 * [Launchpad](https://launchpad.net/)
 * [PECL](https://pecl.php.net/)
 * [AUR](https://aur.archlinux.org/)

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

Alternatively, there is a Python API. There are also ``autocodemeta`` and
``autodoap`` commands that can generate output in the
[codemeta](https://codemeta.github.io/) and
[DOAP](https://github.com/ewilderj/doap) formats, respectively.
