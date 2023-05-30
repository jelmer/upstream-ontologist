build:
	python3 setup.py build_ext -i

check:: unittest

unittest: build
	python3 -m unittest tests.test_suite

coverage: build
	python3 -m coverage run -m unittest tests.test_suite

coverage-html: coverage
	python3 -m coverage html

check:: flake8

flake8:
	flake8 .

check:: typing

typing:
	mypy upstream_ontologist/ tests/
