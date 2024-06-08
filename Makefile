.PHONY: build check unittest coverage coverage-html typing

build:
	python3 setup.py build_ext -i

check:: unittest-py

unittest-py: build
	PYTHONPATH=$(shell pwd)/py python3 -m unittest tests.test_suite

cargo-test:
	cargo test

check:: cargo-test

coverage: build
	PYTHONPATH=$(shell pwd)/py python3 -m coverage run -m unittest tests.test_suite

coverage-html: coverage
	python3 -m coverage html

check:: typing

typing:
	mypy py/ tests/
