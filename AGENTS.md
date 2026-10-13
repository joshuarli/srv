# Agent Guide

`srv` is a Rust HTTP file browser.

- Build: `make build`
- Test: `make test`
- Check: `cargo clippy --all-targets --all-features -- --deny warnings`
- HTTP handling accepts GET requests and closes each connection after one request.
- HTTP and filesystem behavior: [docs/architecture.md](docs/architecture.md).
- Release builds and artifacts: [docs/release.md](docs/release.md).
