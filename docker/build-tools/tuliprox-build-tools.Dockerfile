############################################
# Global args and versions
############################################
ARG RUST_DISTRO=1.90.0-alpine3.22
ARG TRUNK_VER=0.21.14
ARG BINDGEN_VER=0.2.104
ARG CARGO_CHEF_VER=0.1.73
ARG SCCACHE_VER=0.11.0
ARG ALPINE_VER=3.22.2

ARG CARGO_HOME=/usr/local/cargo
ARG BUILDPLATFORM_TAG=latest

############################################
# Build stage to produce ffmpeg resources
############################################
FROM alpine:${ALPINE_VER} AS resources

RUN apk add --no-cache ffmpeg
WORKDIR /src
COPY resources ./resources

RUN ffmpeg -loop 1 -i ./resources/channel_unavailable.jpg -t 10 -r 1 -an \
      -vf "scale=1920:1080" \
      -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
      ./resources/channel_unavailable.ts && \
    ffmpeg -loop 1 -i ./resources/user_connections_exhausted.jpg -t 10 -r 1 -an \
      -vf "scale=1920:1080" \
      -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
      ./resources/user_connections_exhausted.ts && \
    ffmpeg -loop 1 -i ./resources/provider_connections_exhausted.jpg -t 10 -r 1 -an \
      -vf "scale=1920:1080" \
      -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
      ./resources/provider_connections_exhausted.ts && \
    ffmpeg -loop 1 -i ./resources/user_account_expired.jpg -t 10 -r 1 -an \
      -vf "scale=1920:1080" \
      -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
      ./resources/user_account_expired.ts

############################################
# Tool builder runs on TARGETPLATFORM (native, no cross toolchains)
# -> builds trunk/wasm-bindgen/cargo-chef/sccache for that platform
############################################
FROM rust:${RUST_DISTRO} AS toolbuilder

ARG BUILDPLATFORM_TAG
ARG TARGETARCH
ARG TARGETVARIANT

ARG TRUNK_VER
ARG BINDGEN_VER
ARG CARGO_CHEF_VER
ARG SCCACHE_VER
ARG CARGO_HOME

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_HOME=${CARGO_HOME} \
    PATH=${CARGO_HOME}/bin:$PATH \
    SCCACHE_FEATURE_LIST=dist-client,redis,s3,memcached,gcs,azure,gha,webdav,oss,vendored-openssl

# Install build deps for cargo-install (no cross compilers needed on Alpine)
RUN apk add --no-cache \
      bash \
      build-base \
      pkgconf \
      git \
      perl \
      ca-certificates

SHELL ["/bin/bash", "-e", "-u", "-x", "-o", "pipefail", "-c"]

# wasm target required for trunk builds (frontend)
RUN rustup target add wasm32-unknown-unknown

# Build toolchain binaries (native to the image arch; on Alpine this is musl-host)
RUN --mount=type=cache,target=${CARGO_HOME}/registry,id=cargo-registry-${BUILDPLATFORM_TAG}-${TARGETARCH}${TARGETVARIANT},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git,id=cargo-git-${BUILDPLATFORM_TAG}-${TARGETARCH}${TARGETVARIANT},sharing=locked \
    cargo install --locked trunk --version ${TRUNK_VER} --root /out && \
    cargo install --locked wasm-bindgen-cli --version ${BINDGEN_VER} --root /out && \
    cargo install --locked cargo-chef --version ${CARGO_CHEF_VER} --root /out && \
    cargo install --locked sccache \
      --no-default-features --features ${SCCACHE_FEATURE_LIST} \
      --version ${SCCACHE_VER} \
      --root /out

# Strip (best-effort)
RUN strip /out/bin/trunk /out/bin/wasm-bindgen /out/bin/cargo-chef /out/bin/sccache || true

############################################
# Final image runs on TARGETPLATFORM
# -> contains all build deps + rust targets for tuliprox app
############################################
FROM rust:${RUST_DISTRO}

ARG BUILDPLATFORM_TAG
ARG TARGETARCH
ARG TARGETVARIANT

ARG RUST_DISTRO
ARG TRUNK_VER
ARG BINDGEN_VER
ARG CARGO_CHEF_VER
ARG SCCACHE_VER
ARG CARGO_HOME

LABEL io.tuliprox.rust.version="${RUST_DISTRO%%-*}" \
      io.tuliprox.trunk.version="${TRUNK_VER}" \
      io.tuliprox.wasm_bindgen.version="${BINDGEN_VER}" \
      io.tuliprox.cargo_chef.version="${CARGO_CHEF_VER}" \
      io.tuliprox.sccache.version="${SCCACHE_VER}"

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_HOME=${CARGO_HOME} \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=${CARGO_HOME}/bin:$PATH \
    SCCACHE_DIR=/var/cache/sccache \
    RUSTC_WRAPPER=${CARGO_HOME}/bin/sccache

# System deps for building tuliprox (native + wasm)
# - build-base: compiler toolchain (musl host)
# - clang20-dev: libclang for bindgen use-cases
# - binaryen: wasm-opt
# - mold: fast linker (optional; you can enable via RUSTFLAGS outside)
RUN apk add --no-cache \
      bash \
      build-base \
      pkgconf \
      git \
      curl \
      ca-certificates \
      clang20-dev \
      binaryen \
      mold

RUN mkdir -p "${SCCACHE_DIR}"

# wasm target for frontend builds
RUN rustup target add wasm32-unknown-unknown

# Ship tool binaries built in toolbuilder stage
COPY --from=toolbuilder /out/bin/trunk        ${CARGO_HOME}/bin/trunk
COPY --from=toolbuilder /out/bin/wasm-bindgen ${CARGO_HOME}/bin/wasm-bindgen
COPY --from=toolbuilder /out/bin/cargo-chef   ${CARGO_HOME}/bin/cargo-chef
COPY --from=toolbuilder /out/bin/sccache      ${CARGO_HOME}/bin/sccache

# Copy precompiled .ts resources from resources stage
COPY --from=resources /src/resources /src/resources

RUN chmod +x ${CARGO_HOME}/bin/trunk \
              ${CARGO_HOME}/bin/wasm-bindgen \
              ${CARGO_HOME}/bin/cargo-chef \
              ${CARGO_HOME}/bin/sccache

RUN trunk --version \
 && wasm-bindgen --version \
 && cargo-chef --version \
 && sccache --version \
 && mold --version \
 && wasm-opt --version


# note: package translation 
# debian -> alpine 
# libssl-dev    -> libressl-dev (opensll-dev)
# pkg-config    -> pkgconf
# musl-tools    -> musl-dev (build-base) 
# curl          -> curl
# libclang-dev  -> clang21-dev
# binaryen      -> binaryen (community package)
