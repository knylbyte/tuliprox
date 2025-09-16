# For fastser builds we precompile the tools we need for building Rust WASM projects.
# This Dockerfile is used to create a base image with the necessary tools installed.
# It installs the Rust toolchain, the `trunk` build tool, and the `wasm-bindgen-cli` tool.
# The image is based on the official Rust image and uses Debian Bookworm as the base OS.
# The `trunk` tool is used for building and serving Rust WASM applications, while `wasm-bindgen-cli` is used for generating bindings between
# Rust and JavaScript. The image can be extended with Node.js and Yarn if needed for frontend development.
# This prebuild Image will be autoupdated by the CI/CD pipeline on new versions of the tools.

# Rust tools image for Trunk + wasm-bindgen (tagged per architecture)
FROM rust:bookworm

ARG TRUNK_VER=0.21.14
ARG BINDGEN_VER=0.2.101

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

# System dependencies required by Trunk/wasm, plus Binaryen/Clang
RUN --mount=type=cache,target=/var/cache/apt \
    --mount=type=cache,target=/var/lib/apt \
    apt-get update && \
    apt-get install -y --no-install-recommends \
      pkg-config libssl-dev curl ca-certificates libclang-dev binaryen && \
    rm -rf /var/lib/apt/lists/*

# Add the WebAssembly target
RUN rustup target add wasm32-unknown-unknown

# Install pinned tool versions (reproducible builds)
RUN cargo install --locked trunk --version ${TRUNK_VER} && \
    cargo install --locked wasm-bindgen-cli --version ${BINDGEN_VER}

# Quick sanity check (optional)
RUN trunk --version && wasm-bindgen --version
