NAME       := srv
TARGET     := $(shell rustc -vV | awk '/^host:/ {print $$2}')

.PHONY: build release install test test-ci lint

build:
	cargo build

release:
	cargo clean -p $(NAME) --release --target $(TARGET)
	RUSTFLAGS="-Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort" \
	cargo build --release \
	  -Z build-std=std \
	  -Z build-std-features= \
	  --target $(TARGET)

install: release
	cp target/$(TARGET)/release/$(NAME) ~/usr/bin/$(NAME)

test:
	@OUT=$$(cargo test --quiet 2>&1) || { echo "$$OUT"; exit 1; }

test-ci:
	@OUT=$$(cargo test --quiet --release 2>&1) || { echo "$$OUT"; exit 1; }

lint:
	cargo fmt --all
	cargo clippy --fix --allow-dirty --all-targets --all-features -- --deny warnings
