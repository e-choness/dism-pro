# syntax=docker/dockerfile:1
#
# DISM Pro toolchain image (ADR-0003). Two targets:
#   t1 — build / lint / pure tests / benches / cross-compile to *-pc-windows-msvc
#   t2 — t1 + Wine, for `cargo xwin test` of dpro-win logic
#
# Everything is pinned: the base image by digest, Debian packages through a dated
# snapshot.debian.org archive, and the cargo tools by version + SHA-256.
# Changing any pin is its own PR: `build(docker): …`.

# rust:1.99.1-slim-trixie (multi-arch index), resolved 2026-10-22.
ARG RUST_IMAGE=docker.io/library/rust:1.99.1-slim-trixie@sha256:f938561c139ff3f8b4449fe5064ede0ea526229b0fc8ea58a5031c3bf40e72d3

FROM ${RUST_IMAGE} AS t1

# Debian archive state that every `apt-get install` resolves against.
ARG DEBIAN_SNAPSHOT=20261020T000000Z
# Owner decision #5: the Microsoft CRT/SDK license is accepted for Docker builds.
# cargo-xwin accepts it implicitly, so we gate on an explicit opt-in instead.
ARG XWIN_ACCEPT_LICENSE=0
ARG TARGETARCH

ARG CARGO_XWIN_VERSION=0.23.1
ARG SCCACHE_VERSION=0.18.0
ARG CARGO_DENY_VERSION=0.20.2
ARG CARGO_NEXTEST_VERSION=0.9.149

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN rm -f /etc/apt/sources.list.d/debian.sources \
 && printf '%s\n' \
      "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT} trixie main" \
      "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT} trixie-updates main" \
      "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT} trixie-security main" \
      > /etc/apt/sources.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl git xz-utils \
      clang lld llvm \
      valgrind \
 && rm -rf /var/lib/apt/lists/*

# Prebuilt release binaries, verified by SHA-256 (sccache and cargo-deny hashes match
# their upstream .sha256 files; cargo-xwin and nextest don't publish one for these assets).
RUN case "${TARGETARCH}" in \
      amd64) a=x86_64;  nextest_libc=musl \
             xwin_sha=c492c6dfb7e5ac0eee586b796e0fc950ba13077e7f0bbb046f445d71790d5360 \
             sccache_sha=45f1447fbe231e3037bde351ef70677dd212216c8d62ae7ca409fecc4d6acc89 \
             deny_sha=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f \
             nextest_sha=d65c7f14bcb5c1566d1ecd534c5da8dedb81ba58586e478b0ab81c877d2bd8ad ;; \
      arm64) a=aarch64; nextest_libc=gnu \
             xwin_sha=dc0a0fb28fdd90405f69e7365bce91d2504ea7491eb4d79d5655ffa106358925 \
             sccache_sha=2b3284d5da3b46a47dc4229e75bb7b88ac4aa99c8d754fb7d2f84997e5a4354a \
             deny_sha=995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d \
             nextest_sha=6f82e375187cf3a1f3e336879a51940c315a5d0ce7253f7e517ad28295b74189 ;; \
      *) echo "unsupported TARGETARCH=${TARGETARCH}" >&2; exit 1 ;; \
    esac \
 && gh=https://github.com \
 && fetch() { curl -fsSL --retry 3 -o /tmp/t.tgz "$1" && echo "$2  /tmp/t.tgz" | sha256sum -c - \
              && tar -xzf /tmp/t.tgz -C /tmp && rm /tmp/t.tgz; } \
 && fetch "$gh/rust-cross/cargo-xwin/releases/download/v${CARGO_XWIN_VERSION}/cargo-xwin-v${CARGO_XWIN_VERSION}.${a}-unknown-linux-musl.tar.gz" "$xwin_sha" \
 && install -m 0755 /tmp/cargo-xwin /usr/local/cargo/bin/ \
 && fetch "$gh/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-${a}-unknown-linux-musl.tar.gz" "$sccache_sha" \
 && install -m 0755 /tmp/sccache-*/sccache /usr/local/cargo/bin/ \
 && fetch "$gh/EmbarkStudios/cargo-deny/releases/download/${CARGO_DENY_VERSION}/cargo-deny-${CARGO_DENY_VERSION}-${a}-unknown-linux-musl.tar.gz" "$deny_sha" \
 && install -m 0755 /tmp/cargo-deny-*/cargo-deny /usr/local/cargo/bin/ \
 && fetch "$gh/nextest-rs/nextest/releases/download/cargo-nextest-${CARGO_NEXTEST_VERSION}/cargo-nextest-${CARGO_NEXTEST_VERSION}-${a}-unknown-linux-${nextest_libc}.tar.gz" "$nextest_sha" \
 && install -m 0755 /tmp/cargo-nextest /usr/local/cargo/bin/ \
 && rm -rf /tmp/*

RUN rustup component add rustfmt clippy llvm-tools \
 && rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc

# Bake the MSVC CRT + Windows SDK into the image so builds never download it.
# The cache stays writable: cargo-xwin drops tool symlinks into it at run time.
ENV XWIN_CACHE_DIR=/opt/xwin \
    XWIN_ARCH=x86_64,aarch64
RUN if [ "${XWIN_ACCEPT_LICENSE}" != "1" ]; then \
      echo "Set --build-arg XWIN_ACCEPT_LICENSE=1 to accept the Microsoft CRT/SDK license (decisions-log #5)." >&2; \
      exit 1; \
    fi \
 && cargo xwin cache xwin \
 && chmod -R a+rwX /opt/xwin

ENV RUSTC_WRAPPER=sccache \
    SCCACHE_DIR=/sccache \
    CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0
WORKDIR /src

FROM t1 AS t2
# 64-bit Wine only: T2 runs x86_64 test binaries; there are no 32-bit targets.
# apt resolves against the snapshot sources inherited from t1.
RUN apt-get update \
 && apt-get install -y --no-install-recommends wine wine64 \
 && rm -rf /var/lib/apt/lists/*
# mscoree/mshtml disabled: stops the headless prefix from trying to fetch Mono/Gecko.
ENV WINEPREFIX=/opt/wine \
    WINEDEBUG=-all \
    WINEDLLOVERRIDES="mscoree,mshtml=" \
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=wine
# Create the prefix at build time so the first test run doesn't pay for it.
RUN wineboot --init && wineserver --wait && chmod -R a+rwX /opt/wine
