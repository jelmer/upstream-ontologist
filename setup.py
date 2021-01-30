#!/usr/bin/python3

from setuptools import setup

setup(
    name="upstream-ontologist",
    packages=[
        "upstream_ontologist",
        "upstream_ontologist.debian",
        "upstream_ontologist.tests",
    ],
    version="0.1.5",
    author="Jelmer Vernooij",
    author_email="jelmer@debian.org",
    url="https://github.com/jelmer/upstream-ontologist",
    description="tracking of upstream project metadata",
    project_urls={
        "Repository": "https://github.com/jelmer/upstream-ontologist.git",
    },
    requires=['debian', 'debmutate'],
    test_suite="upstream_ontologist.tests.test_suite",
)
