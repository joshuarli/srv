target := arch() + "-apple-darwin"

build:
    cargo build

release:
    cargo clean -p srv --release --target {{ target }}
    RUSTFLAGS="-Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort" \
    cargo +nightly build --release \
      -Z build-std=std \
      -Z build-std-features= \
      --target {{ target }}

install: release
    cp target/{{ target }}/release/srv ~/usr/bin/srv

setup:
  prek install --install-hooks

pc:
  prek run --all-files
