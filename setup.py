#!/usr/bin/python3

from setuptools import setup

setup(
    name="upstream-ontologist",
    packages=[
        "upstream_ontologist",
        "upstream_ontologist.debian",
        "upstream_ontologist.tests",
    ],
    version="0.1.13",
    author="Jelmer Vernooij",
    author_email="jelmer@debian.org",
    url="https://github.com/jelmer/upstream-ontologist",
    description="tracking of upstream project metadata",
    project_urls={
        "Repository": "https://github.com/jelmer/upstream-ontologist.git",
    },
    entry_points={
        'console_scripts': [
            ('guess-upstream-metadata='
             'upstream_ontologist.__main__:main'),
        ],
    },
    install_requires=['python_debian', 'debmutate'],
    extras_require={
        'cargo': ['tomlkit'],
        'readme': ['docutils', 'lxml', 'bs4', 'markdown'],
    },
    tests_require=['breezy'],
    test_suite="upstream_ontologist.tests.test_suite",
)
