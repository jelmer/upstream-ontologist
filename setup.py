#!/usr/bin/python3
from setuptools import setup
from setuptools_rust import Binding, RustBin, RustExtension

setup(
    rust_extensions=[
        RustBin("autodoap", "Cargo.toml"),
        RustBin("autocodemeta", "Cargo.toml"),
        RustExtension(
            "upstream_ontologist._upstream_ontologist",
            "upstream-ontologist-py/Cargo.toml",
            binding=Binding.PyO3,
            features=["rustls-tls", "extension-module"],
        ),
    ],
    data_files=[("share/man/man1", ["man/guess-upstream-metadata.1"])],
)
