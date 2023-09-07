#!/usr/bin/python3
from setuptools import setup
from setuptools_rust import Binding, RustExtension
setup(
    rust_extensions=[RustExtension("upstream_ontologist._upstream_ontologist", "upstream-ontologist-py/Cargo.toml", binding=Binding.PyO3, features=["rustls-tls"])],
    data_files=[('share/man/man1', ['man/guess-upstream-metadata.1'])])
