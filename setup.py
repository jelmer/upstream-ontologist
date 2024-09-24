#!/usr/bin/python3
import sys

from setuptools import setup
from setuptools_rust import Binding, RustExtension

extra_features = []

if sys.platform != "win32":
    extra_features.append("debcargo")

setup(
    rust_extensions=[
        RustExtension(
            "upstream_ontologist._upstream_ontologist",
            "upstream-ontologist-py/Cargo.toml",
            binding=Binding.PyO3,
            features=["extension-module"] + extra_features,
        ),
    ],
    data_files=[("share/man/man1", ["man/guess-upstream-metadata.1"])],
)
