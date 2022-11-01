check:: unittest

unittest:
	python3 -m unittest tests.test_suite

check:: flake8

flake8:
	flake8 .


check:: typing

typing:
	mypy upstream_ontologist/
