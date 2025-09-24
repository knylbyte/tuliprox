# Preinstall all tools needed to speed up both:
# - Stage 1 (native Rust binary; musl on amd64/arm64/armv7)
# - Stage 2 (WASM via trunk + wasm-bindgen)
# No OpenSSL dev packages are needed (the app uses rustls).

############################################
# Global args and versions
############################################
ARG RUST_DISTRO=1.90.0-trixie
ARG TRUNK_VER=0.21.14
ARG BINDGEN_VER=0.2.103
ARG CARGO_CHEF_VER=0.1.72

############################################
# Builder runs on the BUILDPLATFORM (no QEMU)
# -> builds the tool binaries (trunk/wasm-bindgen) for TARGETPLATFORM
############################################
FROM --platform=$BUILDPLATFORM rust:${RUST_DISTRO} AS builder

ARG TARGETPLATFORM
ARG TRUNK_VER
ARG BINDGEN_VER

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

# Map Docker TARGETPLATFORM -> Rust target triple for *tool binaries*.
# Tools must run inside the final image for that platform (gnu is fine here).
RUN case "$TARGETPLATFORM" in \
      "linux/arm/v7")  echo armv7-unknown-linux-gnueabihf  > /rust-target ;; \
      "linux/arm64")   echo aarch64-unknown-linux-gnu      > /rust-target ;; \
      "linux/amd64")   echo x86_64-unknown-linux-gnu       > /rust-target ;; \
      *) echo "Unsupported TARGETPLATFORM: $TARGETPLATFORM" && exit 1 ;; \
    esac

# Cross toolchains so we can produce tool binaries for the platform above
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
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install --locked trunk --version ${TRUNK_VER} \
      --target "$(cat /rust-target)" --root /out && \
    cargo install --locked wasm-bindgen-cli --version ${BINDGEN_VER} \
      --target "$(cat /rust-target)" --root /out && \
    cargo install --locked cargo-chef --version ${CARGO_CHEF_VER} \
      --target "$(cat /rust-target)" --root /out

# Strip (best-effort)
RUN case "$(cat /rust-target)" in \
      armv7-unknown-linux-gnueabihf)  arm-linux-gnueabihf-strip /out/bin/trunk /out/bin/wasm-bindgen /out/bin/cargo-chef || true ;; \
      aarch64-unknown-linux-gnu)      aarch64-linux-gnu-strip   /out/bin/trunk /out/bin/wasm-bindgen /out/bin/cargo-chef || true ;; \
      x86_64-unknown-linux-gnu)       strip                     /out/bin/trunk /out/bin/wasm-bindgen /out/bin/cargo-chef || true ;; \
    esac

############################################
# Final image runs on the TARGETPLATFORM
# -> contains all build deps + rust targets for tuliprox app
############################################
FROM rust:${RUST_DISTRO}

ARG TRUNK_VER
ARG BINDGEN_VER
ARG CARGO_CHEF_VER

LABEL io.tuliprox.trunk.version="${TRUNK_VER}" \
      io.tuliprox.wasm_bindgen.version="${BINDGEN_VER}" \
      io.tuliprox.cargo_chef.version="${CARGO_CHEF_VER}"

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

# System deps for both stages of the app:
# - Stage 1 (native binary): musl-tools (for musl static builds)
# - Stage 2 (WASM): libclang-dev, binaryen
# Keep it lean; no OpenSSL dev packages (we use rustls).
RUN --mount=type=cache,id=apt-final-cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,id=apt-final-lib,target=/var/lib/apt,sharing=locked \
    set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      pkg-config musl-tools \
      curl ca-certificates \
      libclang-dev binaryen; \
    rm -rf /var/lib/apt/lists/*

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

# Quick sanity
RUN chmod +x  /usr/local/cargo/bin/trunk \
              /usr/local/cargo/bin/wasm-bindgen \
              /usr/local/cargo/bin/cargo-chef
RUN trunk --version \
 && wasm-bindgen --version \
 && cargo-chef --version

