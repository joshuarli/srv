FROM alpine:3.24.1

ARG TARGETARCH
ARG LLVM_VERSION=23.1.0-rc2
ARG LLVM_RELEASE_SHA=6eb5fb9
ARG MOLD_VERSION=3.0.0
ARG MOLD_RELEASE_SHA=cfb4aab

RUN apk add --no-cache \
    binutils \
    file \
    make \
    musl-dev \
    libgcc

# rustix requests libutil for Unix PTY support; musl provides those symbols in
# libc, so an empty archive satisfies the legacy library name without adding a
# glibc runtime dependency.
RUN ar rcs /usr/lib/libutil.a

# Use the same prebuilt LLVM family as the Linux CI image. The archive is
# musl-linked so clang, lld, and the LLVM tools run inside Alpine.
ADD https://github.com/laputa-systems/llvm-prebuilt-musl/releases/download/llvm-musl-${LLVM_VERSION}-${LLVM_RELEASE_SHA}/clang+llvm-${LLVM_VERSION}-x86_64-linux-musl.tar.xz /tmp/llvm-x86_64.tar.xz
ADD https://github.com/laputa-systems/llvm-prebuilt-musl/releases/download/llvm-musl-${LLVM_VERSION}-${LLVM_RELEASE_SHA}/clang+llvm-${LLVM_VERSION}-aarch64-linux-musl.tar.xz /tmp/llvm-aarch64.tar.xz
RUN case "$TARGETARCH" in \
        amd64) archive=/tmp/llvm-x86_64.tar.xz ;; \
        arm64) archive=/tmp/llvm-aarch64.tar.xz ;; \
        *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && echo '36647cca0bf57d206a6ce757d07a9d8489ef6ccf283a2cc7f740d1cba99a088b  /tmp/llvm-x86_64.tar.xz' | sha256sum -c - \
    && echo '0c9bd6f0fefa26dbdb7d6ed568f3799b558428b1ce1264656aa328fc6fd9e32d  /tmp/llvm-aarch64.tar.xz' | sha256sum -c - \
    && mkdir -p /opt/llvm-musl \
    && tar xf "$archive" -C /opt/llvm-musl --strip-components=1 \
    && test -f /opt/llvm-musl/lib/libclang.so \
    && rm /tmp/llvm-x86_64.tar.xz /tmp/llvm-aarch64.tar.xz

# mold links the release binaries. It is a musl-linked prerelease build, so it runs
# inside Alpine; clang selects it through --ld-path in musl-cargo.
ADD https://github.com/joshuarli/mold/releases/download/mold-musl-${MOLD_VERSION}-${MOLD_RELEASE_SHA}/mold-${MOLD_VERSION}-x86_64-linux-musl.tar.gz /tmp/mold-x86_64.tar.gz
ADD https://github.com/joshuarli/mold/releases/download/mold-musl-${MOLD_VERSION}-${MOLD_RELEASE_SHA}/mold-${MOLD_VERSION}-aarch64-linux-musl.tar.gz /tmp/mold-aarch64.tar.gz
RUN case "$TARGETARCH" in \
        amd64) archive=/tmp/mold-x86_64.tar.gz ;; \
        arm64) archive=/tmp/mold-aarch64.tar.gz ;; \
        *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && echo 'd6e47d61b74961a60868ad308acad4dfdc199e7ec97d61d65728e6ef1d6c2565  /tmp/mold-x86_64.tar.gz' | sha256sum -c - \
    && echo '2d904b170d804008ada80157f8f18d9920d3ccc1e49e27d80123d01e386668f3  /tmp/mold-aarch64.tar.gz' | sha256sum -c - \
    && mkdir -p /opt/mold \
    && tar xf "$archive" -C /opt/mold --strip-components=1 \
    && /opt/mold/bin/mold --version \
    && rm /tmp/mold-x86_64.tar.gz /tmp/mold-aarch64.tar.gz

RUN for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do \
        stub_dir="/usr/lib/srv-crt/$target"; \
        mkdir -p "$stub_dir"; \
        for obj in crtbegin.o crtbeginS.o crtbeginT.o crtend.o crtendS.o; do \
            /opt/llvm-musl/bin/clang --target="$target" -x c -c /dev/null -o "$stub_dir/$obj"; \
        done; \
    done

ADD https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-musl/rustup-init /rustup-init-x86_64
ADD https://static.rust-lang.org/rustup/dist/aarch64-unknown-linux-musl/rustup-init /rustup-init-aarch64
RUN case "$TARGETARCH" in \
        amd64) init=/rustup-init-x86_64 ;; \
        arm64) init=/rustup-init-aarch64 ;; \
        *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && cp "$init" /rustup-init \
    && chmod +x /rustup-init \
    && /rustup-init -y --default-toolchain none \
    && rm /rustup-init /rustup-init-x86_64 /rustup-init-aarch64

ENV PATH="/opt/llvm-musl/bin:/root/.cargo/bin:$PATH" \
    CC="/opt/llvm-musl/bin/clang" \
    AR="/opt/llvm-musl/bin/llvm-ar" \
    RANLIB="/opt/llvm-musl/bin/llvm-ranlib" \
    LIBRARY_PATH="/opt/llvm-musl/lib" \
    LIBCLANG_PATH="/opt/llvm-musl/lib" \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="/opt/llvm-musl/bin/clang" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="/opt/llvm-musl/bin/clang"

RUN rustup toolchain install nightly-2026-09-15 \
    --target x86_64-unknown-linux-musl \
    --target aarch64-unknown-linux-musl \
    --component rust-src \
    --component llvm-tools-preview

# Host proc-macro links search rustc's target library directory for CRT
# startup objects. Keep those objects available there without changing the
# target's build-std Rust flags.
RUN host_target="$(rustc -vV | awk '/^host:/ {print $2}')" \
    && host_libdir="$(rustc --print target-libdir)" \
    && for obj in crtbegin.o crtbeginS.o crtbeginT.o crtend.o crtendS.o; do \
        ln -sf "/usr/lib/srv-crt/$host_target/$obj" "$host_libdir/$obj"; \
    done

# Host proc-macro crates are dynamically linked while Cargo builds. Make the
# Alpine libc and libgcc_s names visible in rustc's musl host target directory.
RUN host_libdir="$(rustc --print target-libdir)" \
    && ln -sf /usr/lib/libgcc_s.so.1 "$host_libdir/libgcc_s.so" \
    && ln -sf /usr/lib/libgcc_s.so.1 "$host_libdir/libgcc_s.so.1" \
    && ln -sf /usr/lib/libc.so "$host_libdir/libc.so"

# Keep musl linker, CRT, and loader flags in the image. Makefiles select only
# static, dynamic, or test mode through this wrapper, so the same release
# commands work on both container architectures.
RUN cat > /usr/local/bin/musl-cargo <<'EOF' \
    && chmod +x /usr/local/bin/musl-cargo
#!/bin/sh
set -eu

target=${MUSL_TARGET:?MUSL_TARGET is required}
case "$target" in
    x86_64-unknown-linux-musl)
        crt=/usr/lib/srv-crt/x86_64-unknown-linux-musl
        loader=/lib/ld-musl-x86_64.so.1
        ;;
    aarch64-unknown-linux-musl)
        crt=/usr/lib/srv-crt/aarch64-unknown-linux-musl
        loader=/lib/ld-musl-aarch64.so.1
        ;;
    *)
        exec cargo "$@"
        ;;
esac

mode=${MUSL_BUILD_MODE:-static}
native="-L native=/usr/lib -Clink-arg=-B$crt -Clink-arg=--ld-path=/opt/mold/bin/mold"
release="-Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort"
case "$mode" in
    static|test)
        flags=$native
        ;;
    release-static)
        flags="$native $release"
        ;;
    release-dynamic)
        flags="$native $release -Ctarget-feature=-crt-static -Clink-arg=-B$crt -Clink-arg=-dynamic-linker=$loader"
        ;;
    *)
        echo "musl-cargo: unknown build mode: $mode" >&2
        exit 2
        ;;
esac

target_env=$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')
linker_var="CARGO_TARGET_${target_env}_LINKER"
rustflags_var="CARGO_TARGET_${target_env}_RUSTFLAGS"
export "$linker_var=clang"
if [ "$mode" = test ]; then
    # Cargo builds proc-macro and test-support crates for the container host,
    # where target-scoped flags do not apply. Keep those links on the same
    # musl CRT path as the binary.
    unset CARGO_ENCODED_RUSTFLAGS
    export RUSTFLAGS="$flags"
else
    export "$rustflags_var=$flags"
fi
exec cargo "$@"
EOF
