# For fastser builds we precompile the tools we need for building Rust WASM projects.
# This Dockerfile is used to create a base image with the necessary tools installed.
# It installs the Rust toolchain, the `trunk` build tool, and the `wasm-bindgen-cli` tool.
# The image is based on the official Rust image and uses Debian Bookworm as the base OS.
# The `trunk` tool is used for building and serving Rust WASM applications, while `wasm-bindgen-cli` is used for generating bindings between
# Rust and JavaScript. The image can be extended with Node.js and Yarn if needed for frontend development.
# This prebuild Image will be autoupdated by the CI/CD pipeline on new versions of the tools.

############################################
# Global args and settings
############################################

ARG RUST_DISTRO=bookworm
ARG TRUNK_VER=0.21.14
ARG BINDGEN_VER=0.2.101

############################################
# Builder runs on the BUILDPLATFORM (no QEMU)
############################################
FROM --platform=$BUILDPLATFORM rust:${RUST_DISTRO} AS builder

ARG TRUNK_VER
ARG BINDGEN_VER

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

# Map Docker TARGETPLATFORM -> Rust target triple
# Extend if you add more platforms later
RUN case "$TARGETPLATFORM" in \
      "linux/arm/v7")  echo armv7-unknown-linux-gnueabihf  > /rust-target ;; \
      "linux/arm64")   echo aarch64-unknown-linux-gnu      > /rust-target ;; \
      "linux/amd64")   echo x86_64-unknown-linux-gnu       > /rust-target ;; \
      *) echo "Unsupported TARGETPLATFORM: $TARGETPLATFORM" && exit 1 ;; \
    esac

# Minimal toolchains to cross-compile Rust binaries
# (armv7/arm64 linkers; amd64 uses native strip)
# Builder stage (runs on BUILDPLATFORM)
RUN --mount=type=cache,id=apt-builder-cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,id=apt-builder-lib,target=/var/lib/apt,sharing=locked \
    set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      curl ca-certificates pkg-config \
      gcc-arm-linux-gnueabihf binutils-arm-linux-gnueabihf \
      gcc-aarch64-linux-gnu    binutils-aarch64-linux-gnu; \
    case "$(cat /rust-target)" in \
      armv7-unknown-linux-gnueabihf) \
        apt-get install -y --no-install-recommends \
          libc6-dev-armhf-cross linux-libc-dev-armhf-cross ;; \
      aarch64-unknown-linux-gnu) \
        apt-get install -y --no-install-recommends \
          libc6-dev-arm64-cross linux-libc-dev-arm64-cross ;; \
      *) : ;; \
    esac; \
    rm -rf /var/lib/apt/lists/*

# Add std for wasm32 and the native target triple we compile for
RUN rustup target add wasm32-unknown-unknown $(cat /rust-target)

# Tell cargo which linker to use for cross targets
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# Speed up builds for tool binaries (they're not perf-critical)
ENV CARGO_PROFILE_RELEASE_CODEGEN_UNITS=64 \
    CARGO_PROFILE_RELEASE_LTO=off \
    CARGO_PROFILE_RELEASE_DEBUG=false \
    CARGO_PROFILE_RELEASE_OPT_LEVEL=2

# Build tool binaries for the target platform (no QEMU)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install --locked trunk --version ${TRUNK_VER} \
      --target "$(cat /rust-target)" --root /out && \
    cargo install --locked wasm-bindgen-cli --version ${BINDGEN_VER} \
      --target "$(cat /rust-target)" --root /out

# Strip binaries to reduce size (best-effort)
RUN case "$(cat /rust-target)" in \
      armv7-unknown-linux-gnueabihf)  arm-linux-gnueabihf-strip /out/bin/trunk /out/bin/wasm-bindgen || true ;; \
      aarch64-unknown-linux-gnu)      aarch64-linux-gnu-strip   /out/bin/trunk /out/bin/wasm-bindgen || true ;; \
      x86_64-unknown-linux-gnu)       strip                     /out/bin/trunk /out/bin/wasm-bindgen || true ;; \
    esac

############################################
# Final image runs on the TARGETPLATFORM
############################################
FROM rust:${RUST_DISTRO}

ARG TRUNK_VER
ARG BINDGEN_VER

LABEL io.tuliprox.trunk.version="${TRUNK_VER}" \
      io.tuliprox.wasm_bindgen.version="${BINDGEN_VER}"

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

# System dependencies required by Trunk/wasm + Binaryen/Clang
RUN --mount=type=cache,id=apt-final-cache,target=/var/cache/apt \
    --mount=type=cache,id=apt-final-lib,target=/var/lib/apt \
    apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev curl ca-certificates libclang-dev binaryen && \
    rm -rf /var/lib/apt/lists/*

# Add the wasm target (used by downstream builds)
RUN rustup target add wasm32-unknown-unknown

# Copy the prebuilt tool binaries from builder
COPY --from=builder /out/bin/trunk /usr/local/cargo/bin/trunk
COPY --from=builder /out/bin/wasm-bindgen /usr/local/cargo/bin/wasm-bindgen

# Quick sanity check
RUN chmod +x /usr/local/cargo/bin/trunk /usr/local/cargo/bin/wasm-bindgen \
 && trunk --version && wasm-bindgen --version
