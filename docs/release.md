# Releases

The `release` GitHub Actions workflow runs manually through `workflow_dispatch`. It builds macOS ARM64 and Linux x86_64/AArch64 binaries. Linux builds run in the `Dockerfile` image and produce both dynamic and static musl binaries. The workflow verifies Linux binaries, zips the artifacts, and publishes a GitHub prerelease named `release-<commit-sha>`.

To build locally, use the pinned nightly toolchain in `rust-toolchain.toml`:

- `make release TARGET=<rust-target>` builds a static release binary.
- `make release-dynamic TARGET=<rust-target>` builds a dynamic release binary.
- `make verify-release TARGET=<rust-target>` or `make verify-release-dynamic TARGET=<rust-target>` checks the corresponding artifact. Linux checks require `readelf`; non-Linux targets only check that the file exists.

Linux musl builds use the `musl-cargo` wrapper supplied by the release image. Release artifacts are written under `target/<rust-target>/release/`.
