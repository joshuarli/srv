NAME       := srv
HOST       := $(shell rustc -vV | awk '/^host:/ {print $$2}')
TARGET     ?= $(subst -unknown-linux-gnu,-unknown-linux-musl,$(HOST))
TARGET_ENV := $(shell printf '%s' "$(TARGET)" | tr '[:lower:]-' '[:upper:]_')
CARGO_CMD  := $(if $(findstring -linux-musl,$(TARGET)),$(shell command -v musl-cargo 2>/dev/null || printf cargo),cargo)
comma      := ,
# Setting RUSTFLAGS replaces the target rustflags from .cargo/config.toml, so the
# macOS unused-dylib link argument is repeated for release builds.
RELEASE_RUSTFLAGS := -Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort $(if $(findstring apple-darwin,$(TARGET)),-Clink-arg=-Wl$(comma)-dead_strip_dylibs)
# Docker's musl-cargo wrapper owns linker, CRT, and loader flags. The Makefile
# selects only the build mode and keeps Cargo invocations readable on macOS as
# well as in the Linux release image.
cargo = $(if $(findstring -linux-musl,$(TARGET)),,$(if $(filter release-static release-dynamic test,$(1)),CARGO_TARGET_$(TARGET_ENV)_LINKER=clang ,)$(if $(filter release-static release-dynamic,$(1)),RUSTFLAGS="$(RELEASE_RUSTFLAGS)")) MUSL_TARGET="$(TARGET)" MUSL_BUILD_MODE="$(1)" $(CARGO_CMD)

.PHONY: build test test-ci lint release release-dynamic verify-release verify-release-dynamic install

build:
	$(call cargo,dev) build

test:
	$(call cargo,test) test --quiet

test-ci:
	$(call cargo,test) test --quiet --release

lint:
	cargo clippy --fix --allow-dirty --all-targets --all-features -- --deny warnings

release:
	$(call cargo,release-static) clean -p $(NAME) --release --target $(TARGET)
	$(call cargo,release-static) build --release \
	  -Z build-std=std \
	  -Z build-std-features= \
	  --target $(TARGET)

verify-release:
	@test -f "target/$(TARGET)/release/$(NAME)"
	@if echo "$(TARGET)" | grep -q -- '-linux-musl$$'; then \
		command -v readelf >/dev/null || { echo 'readelf is required for release verification'; exit 1; }; \
		file "target/$(TARGET)/release/$(NAME)" | grep -Eq 'static-pie linked|statically linked' || { echo 'release is not statically linked'; exit 1; }; \
		file "target/$(TARGET)/release/$(NAME)" | grep -q 'stripped' || { echo 'release is not stripped'; exit 1; }; \
		! readelf -l "target/$(TARGET)/release/$(NAME)" | grep -q INTERP || { echo 'release has a dynamic ELF interpreter'; exit 1; }; \
		! readelf -d "target/$(TARGET)/release/$(NAME)" | grep -q NEEDED || { echo 'release has dynamic dependencies'; exit 1; }; \
	else \
		echo "Skipping ELF checks for $(TARGET)"; \
	fi

verify-release-dynamic:
	@test -f "target/$(TARGET)/release/$(NAME)"
	@if echo "$(TARGET)" | grep -q -- '-linux-musl$$'; then \
		command -v readelf >/dev/null || { echo 'readelf is required for release verification'; exit 1; }; \
		file "target/$(TARGET)/release/$(NAME)" | grep -q 'dynamically linked' || { echo 'release is not dynamically linked'; exit 1; }; \
		file "target/$(TARGET)/release/$(NAME)" | grep -q 'stripped' || { echo 'release is not stripped'; exit 1; }; \
		readelf -l "target/$(TARGET)/release/$(NAME)" | grep -q '/lib/ld-musl-' || { echo 'release does not use the musl loader'; exit 1; }; \
		readelf -d "target/$(TARGET)/release/$(NAME)" | grep -q NEEDED || { echo 'release has no dynamic dependencies'; exit 1; }; \
	else \
		echo "Skipping ELF checks for $(TARGET)"; \
	fi

release-dynamic:
	$(call cargo,release-dynamic) clean -p $(NAME) --release --target $(TARGET)
	$(call cargo,release-dynamic) build --release \
	  -Z build-std=std \
	  -Z build-std-features= \
	  --target $(TARGET)

install: release
	cp target/$(TARGET)/release/$(NAME) ~/usr/bin/$(NAME)
	@if test "$$(uname -s)" = Darwin; then \
		codesign -fs - ~/usr/bin/$(NAME); \
	fi
