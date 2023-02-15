#!/bin/sh

set -eux
repack_version="$1"
version="${repack_version%+repack*}"
tag="v$(echo "$version" | tr '~' '.')"
tmpdir=$(mktemp -d -t exampl.get-orig-source.XXXXXX)
orig_dir="exampl-${version}+repack.orig"
git clone -b "$tag" --depth 1 https://example.com/scm/project.git "$tmpdir/${orig_dir}"
rm -rf "$tmpdir"/*.orig/src/tls/ # free, but appears to be an unused code example from gnutls
export TAR_OPTIONS='--owner root --group root --mode a+rX --format ustar'
tar -cJ --wildcards --exclude '.git*' -C "$tmpdir/" "${orig_dir}" \
> "../exampl_${version}+repack.orig.tar.xz"
rm -rf "$tmpdir"

# vim:ts=4 sw=4 et
