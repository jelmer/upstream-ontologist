#!/usr/bin/python3

from setuptools import setup

setup(
    name="upstream-ontologist",
    packages=["upstream_ontologist"],
    version="0.1.3",
    author="Jelmer Vernooij",
    author_email="jelmer@debian.org",
    url="https://github.com/jelmer/upstream-ontologist",
    description="tracking of upstream project metadata",
    project_urls={
        "Repository": "https://github.com/jelmer/upstream-ontologist.git",
    },
    test_suite="upstream_ontologist.tests.test_suite",
)
