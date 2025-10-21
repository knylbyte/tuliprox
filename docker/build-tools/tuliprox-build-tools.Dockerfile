# Preinstall all tools needed to speed up final image builds:
# - Stage 1 (build precompiled .ts resources via ffmpeg)
# - Stage 2 (native Rust binary; musl on amd64/arm64/armv7)
# - Stage 3 (WASM via trunk + wasm-bindgen)
# No OpenSSL dev packages are needed (the app uses rustls).
# sccache is built with all upstream features enabled
# (dist-client, redis, s3, memcached, gcs, azure, gha, webdav, oss) and vendored OpenSSL.
# This avoids cross-compiling OpenSSL system libraries for the build-tools image.
# The sccache cache dir is /var/cache/sccache.

############################################
# Global args and versions
############################################
ARG RUST_DISTRO=1.90.0-trixie \
    TRUNK_VER=0.21.14 \
    BINDGEN_VER=0.2.104 \
    CARGO_CHEF_VER=0.1.73 \
    CARGO_MACHETE_VER=0.9.1 \
    SCCACHE_VER=0.11.0 \
    ALPINE_VER=3.22.2 \
    CARGO_HOME=/usr/local/cargo \
    SCCACHE_DIR=/var/cache/sccache \
    BUILDPLATFORM_TAG=latest
############################################
# Build stage to produce ffmpeg resources
# -> contains prebuilt .ts files from .jpg
############################################
FROM alpine:${ALPINE_VER} AS resources

RUN apk add --no-cache ffmpeg
WORKDIR /src
COPY resources ./resources

# Combine ffmpeg commands into a single layer to reduce image size
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
# Builder runs on the BUILDPLATFORM (no QEMU)
# -> builds the tool binaries (trunk/wasm-bindgen) for TARGETPLATFORM
############################################
FROM --platform=$BUILDPLATFORM rust:${RUST_DISTRO} AS builder

SHELL ["/bin/bash", "-e", "-u", "-x", "-o", "pipefail", "-c"]

ARG BUILDPLATFORM_TAG \
    TARGETPLATFORM  \
    TRUNK_VER \
    BINDGEN_VER \
    CARGO_CHEF_VER \
    CARGO_MACHETE_VER \
    SCCACHE_VER \
    CARGO_HOME \
    SCCACHE_DIR

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=${CARGO_HOME} \
    PATH=${CARGO_HOME}:$PATH \
    SCCACHE_DIR=${SCCACHE_DIR} \
    SCCACHE_FEATURE_LIST=dist-client,redis,s3,memcached,gcs,azure,gha,webdav,oss,vendored-openssl

# Map Docker TARGETPLATFORM -> Rust target triple for *tool binaries*.
# Tools must run inside the final image for that platform (gnu is fine here).
RUN case "$TARGETPLATFORM" in \
      "linux/arm/v7")  echo armv7-unknown-linux-gnueabihf  > /rust-target ;; \
      "linux/arm64")   echo aarch64-unknown-linux-gnu      > /rust-target ;; \
      "linux/amd64")   echo x86_64-unknown-linux-gnu       > /rust-target ;; \
      *) echo "Unsupported TARGETPLATFORM: $TARGETPLATFORM" && exit 1 ;; \
    esac

# Cross toolchains so we can produce tool binaries for the platform above
RUN --mount=type=cache,target=/var/cache/apt,id=var-cache-apt-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=/var/lib/apt,id=var-lib-apt-${BUILDPLATFORM_TAG},sharing=locked \
    rm -f /etc/apt/apt.conf.d/docker-clean; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      curl ca-certificates pkg-config make perl \
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
    esac

# Targets required for tool builds
RUN rustup target add wasm32-unknown-unknown $(cat /rust-target)

# Linkers for cross tool builds
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# Speed up (tools are not perf-critical)
ENV CARGO_PROFILE_RELEASE_CODEGEN_UNITS=64 \
    CARGO_PROFILE_RELEASE_LTO=off \
    CARGO_PROFILE_RELEASE_DEBUG=false \
    CARGO_PROFILE_RELEASE_OPT_LEVEL=2

# Build trunk & wasm-bindgen for the platform-specific tool image
RUN --mount=type=cache,target=${CARGO_HOME}/registry,id=cargo-registry-${BUILDPLATFORM_TAG} \
    --mount=type=cache,target=${CARGO_HOME}/git,id=cargo-git-${BUILDPLATFORM_TAG} \

    cargo install --locked trunk --version ${TRUNK_VER} \
      --target "$(cat /rust-target)" --root /out; \
    cargo install --locked wasm-bindgen-cli --version ${BINDGEN_VER} \
      --target "$(cat /rust-target)" --root /out; \
    cargo install --locked cargo-chef --version ${CARGO_CHEF_VER} \
      --target "$(cat /rust-target)" --root /out; \
    cargo install --locked cargo-machete --version ${CARGO_MACHETE_VER} \
      --target "$(cat /rust-target)" --root /out; \
    cargo install --locked sccache --no-default-features --features ${SCCACHE_FEATURE_LIST} \
      --target "$(cat /rust-target)" \
      --root /out \
      --version ${SCCACHE_VER}

### TESTING: build sccache from custom fork
# force: true
# WORKDIR /tmp

# RUN --mount=type=cache,target=${CARGO_HOME}/registry,id=cargo-registry-${BUILDPLATFORM_TAG} \
#     --mount=type=cache,target=${CARGO_HOME}/git,id=cargo-git-${BUILDPLATFORM_TAG} \
# 
#     apt-get install -y git; \
#     git clone https://github.com/knylbyte/sccache.git -b main; \
#     cd sccache; \
#     cargo install --locked --path .\
#       --no-default-features --features ${SCCACHE_FEATURE_LIST} \
#       --target "$(cat /rust-target)" \
#       --root /out; \
#     rm -rf /tmp/sccache
### End TESTING

# Strip (best-effort)
RUN case "$(cat /rust-target)" in \
      armv7-unknown-linux-gnueabihf)  arm-linux-gnueabihf-strip /out/bin/trunk /out/bin/wasm-bindgen /out/bin/cargo-chef /out/bin/cargo-machete /out/bin/sccache || true ;; \
      aarch64-unknown-linux-gnu)      aarch64-linux-gnu-strip   /out/bin/trunk /out/bin/wasm-bindgen /out/bin/cargo-chef /out/bin/cargo-machete /out/bin/sccache || true ;; \
      x86_64-unknown-linux-gnu)       strip                     /out/bin/trunk /out/bin/wasm-bindgen /out/bin/cargo-chef /out/bin/cargo-machete /out/bin/sccache || true ;; \
    esac

############################################
# Final image runs on the TARGETPLATFORM
# -> contains all build deps + rust targets for tuliprox app
############################################
FROM rust:${RUST_DISTRO}

SHELL ["/bin/bash", "-e", "-u", "-x", "-o", "pipefail", "-c"]

ARG BUILDPLATFORM_TAG \
    RUST_DISTRO \
    TRUNK_VER \
    BINDGEN_VER \
    CARGO_CHEF_VER \
    CARGO_MACHETE_VER \
    SCCACHE_VER \
    CARGO_HOME \
    SCCACHE_DIR

LABEL io.tuliprox.rust.version="${RUST_DISTRO%%-*}" \
      io.tuliprox.trunk.version="${TRUNK_VER}" \
      io.tuliprox.wasm_bindgen.version="${BINDGEN_VER}" \
      io.tuliprox.cargo_chef.version="${CARGO_CHEF_VER}" \
      io.tuliprox.cargo_machete.version="${CARGO_MACHETE_VER}" \
      io.tuliprox.sccache.version="${SCCACHE_VER}"

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=${CARGO_HOME} \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=${CARGO_HOME}:$PATH \
    SCCACHE_DIR=${SCCACHE_DIR} \
    RUSTC_WRAPPER=${CARGO_HOME}/bin/sccache

RUN mkdir -p \
  ${SCCACHE_DIR} \
  ${CARGO_HOME} \
  ${RUSTUP_HOME}

# System deps for both stages of the app:
# - Stage 1 (native binary): musl-tools (for musl static builds)
# - Stage 2 (WASM): libclang-dev, binaryen
# Keep it lean; no OpenSSL dev packages (we use rustls).
RUN --mount=type=cache,target=/var/cache/apt,id=var-cache-apt-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=/var/lib/apt,id=var-lib-apt-${BUILDPLATFORM_TAG},sharing=locked \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      pkg-config musl-tools \
      curl ca-certificates \
      libclang-dev binaryen

# Add rust targets used by the application:
# - wasm32 (frontend)
# - musl on amd64/arm64/armv7 (static)
RUN rustup target add \
      wasm32-unknown-unknown \
      x86_64-unknown-linux-musl \
      aarch64-unknown-linux-musl \
      armv7-unknown-linux-musleabihf

# Tell cargo which C compiler/linker to use for musl targets
# (when building *inside* the platform-native tool image)
ENV CC_x86_64_unknown_linux_musl=musl-gcc \
    CC_aarch64_unknown_linux_musl=musl-gcc \
    CC_armv7_unknown_linux_musleabihf=musl-gcc \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
    CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER=musl-gcc

# Ship tool binaries built in the builder stage
COPY --from=builder /out/bin/trunk /usr/local/cargo/bin/trunk
COPY --from=builder /out/bin/wasm-bindgen /usr/local/cargo/bin/wasm-bindgen
COPY --from=builder /out/bin/cargo-chef /usr/local/cargo/bin/cargo-chef
COPY --from=builder /out/bin/cargo-machete /usr/local/cargo/bin/cargo-machete
COPY --from=builder /out/bin/sccache /usr/local/cargo/bin/sccache

# Copy precompiled .ts resources from resources stage
COPY --from=resources /src/resources /src/resources

# Quick sanity
RUN chmod +x  /usr/local/cargo/bin/trunk \
              /usr/local/cargo/bin/wasm-bindgen \
              /usr/local/cargo/bin/cargo-chef \
              /usr/local/cargo/bin/cargo-machete \
              /usr/local/cargo/bin/sccache

RUN trunk --version \
 && wasm-bindgen --version \
 && cargo-chef --version \
 && cargo machete --version \
 && sccache --version


