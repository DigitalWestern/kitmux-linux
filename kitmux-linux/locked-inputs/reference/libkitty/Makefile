ROOT := $(abspath ..)
CC := clang
PYTHON_CONFIG ?= $(ROOT)/scripts/python-config.sh
PYTHON_HOME ?= $(shell $(ROOT)/scripts/python-runtime.sh home)
MACOSX_DEPLOYMENT_TARGET ?= 13.0
export MACOSX_DEPLOYMENT_TARGET
PY_CFLAGS  := $(shell $(PYTHON_CONFIG) --embed --cflags)
PY_LDFLAGS := $(shell $(PYTHON_CONFIG) --embed --ldflags)
# -UNDEBUG: python3-config injects -DNDEBUG, which would compile out the
# tests' asserts and let them pass vacuously.
CFLAGS := -Wall -Wextra -g -Iinclude $(PY_CFLAGS) -UNDEBUG

KITTY_SRC := $(ROOT)/vendor/kitty
LIBKITTY_PY := $(abspath py)
TEST_ENV := KITTY_SRC=$(KITTY_SRC) LIBKITTY_PY=$(LIBKITTY_PY) PYTHONHOME=$(PYTHON_HOME)
TEST_CONFIG := $(abspath tests/fixtures/kitty.conf)

SRCS := $(wildcard src/*.c)
TESTS := test_engine test_session test_render_smoke test_render_multi_context

runtime:
	$(ROOT)/scripts/build-kitty.sh

libkitty.dylib: runtime $(SRCS) include/libkitty.h
	$(CC) -dynamiclib -o $@ $(SRCS) $(CFLAGS) $(PY_LDFLAGS) \
		-install_name @rpath/libkitty.dylib

test_%: tests/test_%.c libkitty.dylib
	$(CC) -o $@ $< $(CFLAGS) -L. -lkitty -Wl,-rpath,@loader_path \
		$(if $(findstring render,$@),-framework OpenGL)

test: $(TESTS)
	$(TEST_ENV) ./test_engine
	$(TEST_ENV) LIBKITTY_TEST_CONFIG=$(TEST_CONFIG) ./test_session
	$(TEST_ENV) LIBKITTY_TEST_CONFIG=$(TEST_CONFIG) ./test_render_smoke
	$(TEST_ENV) LIBKITTY_TEST_CONFIG=$(TEST_CONFIG) ./test_render_multi_context

example: examples/minimal_host.m libkitty.dylib
	$(CC) -o minimal_host examples/minimal_host.m $(CFLAGS) -L. -lkitty \
		-Wl,-rpath,@loader_path -framework Cocoa -framework OpenGL

run-example: example
	$(TEST_ENV) ./minimal_host

dist: libkitty.dylib
	rm -rf dist && mkdir -p dist/include dist/py
	cp libkitty.dylib dist/
	cp include/libkitty.h dist/include/
	cp py/glue.py dist/py/
	@echo "dist/ ready (Python runtime + built kitty checkout still required; see README)"

clean:
	rm -rf libkitty.dylib libkitty.dylib.dSYM $(TESTS) minimal_host dist *.dSYM

.PHONY: runtime test example run-example dist clean
