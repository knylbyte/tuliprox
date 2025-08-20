# For fastser builds we precompile the tools we need for building Rust WASM projects.
# This Dockerfile is used to create a base image with the necessary tools installed.
# It installs the Rust toolchain, the `trunk` build tool, and the `wasm-bindgen-cli` tool.
# The image is based on the official Rust image and uses Debian Bookworm as the base OS.
# The `trunk` tool is used for building and serving Rust WASM applications, while `wasm-bindgen-cli` is used for generating bindings between
# Rust and JavaScript. The image can be extended with Node.js and Yarn if needed for frontend development.
# This prebuild Image will be autoupdated by the CI/CD pipeline on new versions of the tools.

FROM rust:bookworm

ARG TRUNK_VER=0.21.13
ARG BINDGEN_VER=0.2.99

RUN rustup target add wasm32-unknown-unknown \
 && cargo install --locked trunk${TRUNK_VER:+@${TRUNK_VER}} \
 && cargo install --locked wasm-bindgen-cli${BINDGEN_VER:+@${BINDGEN_VER}}

# (Optional) Node/Yarn, if your front end needs it
# RUN apt-get update && apt-get install -y --no-install-recommends nodejs npm && rm -rf /var/lib/apt/lists/*
