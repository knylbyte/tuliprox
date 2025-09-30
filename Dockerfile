# =================================================================
#
# Part 1: Prerequisite Build Stages
#
# These stages are used as building blocks for the final images.
# They prepare the Rust binary, the Node.js frontend, and other resources.
#
# - Uses the prebuild image: ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG}
# - Deterministic dep caching via cargo-chef (deps as Docker layers)
# - Sparse index for crates.io
#
# =================================================================

# -----------------------------------------------------------------
# Global configuration (override with --build-arg ...)
# -----------------------------------------------------------------
ARG GHCR_NS=ghcr.io/euzu/tuliprox
ARG BUILDPLATFORM_TAG=latest
ARG ALPINE_VER=3.22.1
ARG RUST_ALPINE_TAG=alpine
ARG DEFAULT_TZ=UTC

# =============================================================================
# Stage 0: chef  (cargo-chef prepare)
#  - Generates a recipe.json that represents all Rust dependencies
#  - Computes /rust-target from TARGETPLATFORM if RUST_TARGET not set
# =============================================================================
FROM ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG} AS chef

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

WORKDIR /src

# Map TARGETPLATFORM -> RUST_TARGET (musl for scratch)
# - amd64  -> x86_64-unknown-linux-musl
# - arm64  -> aarch64-unknown-linux-musl
# - arm/v7 -> armv7-unknown-linux-musleabihf
RUN set -eux; \
    if [ -z "${RUST_TARGET:-}" ]; then \
      case "$TARGETPLATFORM" in \
        "linux/amd64")  echo x86_64-unknown-linux-musl        > /rust-target ;; \
        "linux/arm64")  echo aarch64-unknown-linux-musl       > /rust-target ;; \
        "linux/arm/v7") echo armv7-unknown-linux-musleabihf   > /rust-target ;; \
        *) echo "Unsupported TARGETPLATFORM: $TARGETPLATFORM" >&2; exit 1 ;; \
      esac; \
    else \
      echo "${RUST_TARGET}" > /rust-target; \
    fi; \
    printf "Using RUST_TARGET=%s\n" "$(cat /rust-target)"

# Prepare dependency recipe (reacts to Cargo.toml/Cargo.lock changes)
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# =============================================================================
# Stage 1: deps (cargo-chef cook)
#  - Builds ONLY dependencies as cacheable Docker layers
#  - No cache-mounts here → proper cross-run caching via buildx
# =============================================================================
FROM ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG} AS deps

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
WORKDIR /src

COPY --from=chef /rust-target   /rust-target
COPY --from=chef /src/recipe.json /src/recipe.json

RUN rustup target add "$(cat /rust-target)" || true
RUN set -eux; \
    cargo chef cook --release --target "$(cat /rust-target)" --recipe-path recipe.json

# =============================================================================
# Stage 2: rust-build (application code)
#  - Reuses compiled deps from Stage 1
#  - Builds backend binary statically (musl)
# =============================================================================
FROM ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG} AS rust-build

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    RUSTFLAGS='--remap-path-prefix $HOME=~ -C target-feature=+crt-static'

WORKDIR /src

# Reuse dependency artifacts from deps stage
COPY --from=deps /usr/local/cargo /usr/local/cargo
COPY --from=deps /usr/local/rustup /usr/local/rustup
COPY --from=deps /src/target       /src/target
COPY --from=chef /rust-target      /rust-target

# Actual app sources
COPY . .

# Ensure target available (no-op if already present)
RUN rustup target add "$(cat /rust-target)" || true

# Build the backend (assuming package name is 'tuliprox')
RUN set -eux; \
    cargo build -p tuliprox --target "$(cat /rust-target)" --release --locked

# -----------------------------------------------------------------
# Stage 2: Build the rust frontend (uses prebuild)
# -----------------------------------------------------------------
FROM ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG} AS trunk-build

# Default WASM target used by trunk/cargo
ARG RUST_TARGET=wasm32-unknown-unknown

# 
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

# Work in workspace root
WORKDIR /src

# Prime cargo cache for WASM deps (copy manifests only)
# This allows compiling dependencies without copying the whole source tree.
COPY Cargo.toml Cargo.lock ./
COPY frontend/Cargo.toml ./frontend/
COPY shared/Cargo.toml   ./shared/
COPY backend/Cargo.toml  ./backend/

# Create dummy sources so cargo can build dependency graph only
# (no need to compile your actual app code yet)
RUN set -eux; \
    mkdir -p backend/src frontend/src shared/src && \
    echo 'fn main() {}'      > backend/src/main.rs && \
    echo "fn main() {}"      > frontend/src/main.rs && \
    echo "pub fn dummy() {}" > frontend/src/lib.rs  && \
    echo "pub fn dummy() {}" > shared/src/lib.rs

# Use sparse protocol for crates.io registry to reduce data transfer
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

# Build only dependencies for the frontend crate on the WASM target
# Result: registry/git caches + compiled deps kept in image layers.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-trunk,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-trunk,sharing=locked \
    set -eux; \
    cargo build --manifest-path frontend/Cargo.toml \
                --target ${RUST_TARGET} \
                --release || true

# Now copy the real sources and perform the actual trunk build
COPY . .
WORKDIR /src/frontend

# Trunk will reuse the warmed cargo caches from the steps above.
RUN set -eux; \
    trunk build --release
# dist -> /src/frontend/dist

# -----------------------------------------------------------------
# Stage 3: tzdata/zoneinfo supplier (shared)
# -----------------------------------------------------------------
FROM alpine:${ALPINE_VER} AS tzdata
RUN set -eux; \
    apk add --no-cache tzdata ca-certificates; \
    update-ca-certificates; \
    test -d /usr/share/zoneinfo

# -----------------------------------------------------------------
# Stage 4: Resources (prebuilt ffmpeg outputs)
# -----------------------------------------------------------------
FROM ${GHCR_NS}/resources:${BUILDPLATFORM_TAG} AS resources
# Expected: /src/resources/*.ts

# =================================================================
#
# Part 2: Final Image Stages
#
# These stages build the final, runnable images by assembling
# the artifacts from the prerequisite stages above.
#
# =================================================================

# -----------------------------------------------------------------
# Final Image #1: Final runtime (FROM scratch) -> all musl targets
# -----------------------------------------------------------------
FROM scratch AS scratch-final

ARG DEFAULT_TZ=UTC
ENV TZ=${DEFAULT_TZ}

# Put runtime data under /opt/tuliprox/data (default landing dir)
WORKDIR /opt/tuliprox/data

# Copy zoneinfo + CA store for TLS & timezones
COPY --from=tzdata /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=tzdata /etc/ssl/certs      /etc/ssl/certs

# Copy binary & assets into /opt tree
COPY --from=rust-build  /src/target/*/release/tuliprox /opt/tuliprox/bin/tuliprox
COPY --from=trunk-build /src/frontend/dist             /opt/tuliprox/web/dist
COPY --from=resources   /src/resources                 /opt/tuliprox/resources

# In scratch we cannot create symlinks (no shell); duplicate to PATH location
COPY --from=rust-build  /src/target/*/release/tuliprox /usr/local/bin/tuliprox

EXPOSE 8901
ENTRYPOINT ["/opt/tuliprox/bin/tuliprox"]
CMD ["-s", "-p", "/opt/tuliprox/data"]

# -----------------------------------------------------------------
# Final Image #2: Final runtime (FROM Alpine) -> dev-friendly
# -----------------------------------------------------------------
FROM alpine:${ALPINE_VER} AS alpine-final

ARG DEFAULT_TZ=UTC
ENV TZ=${DEFAULT_TZ}

# Dev tooling: bash, curl, tshark
# (tshark may require --cap-add NET_ADMIN --cap-add NET_RAW and often --network host)
RUN set -eux; \
    apk add --no-cache ca-certificates bash curl tshark; \
    update-ca-certificates || true

# Layout under /opt (root-owned)
RUN set -eux; \
    mkdir -p \
    /opt/tuliprox/bin \
    /opt/tuliprox/data \
    /opt/tuliprox/web \
    /opt/tuliprox/resources

# Copy zoneinfo & CA store
COPY --from=tzdata /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=tzdata /etc/ssl/certs      /etc/ssl/certs

# Copy binary & assets
COPY --from=rust-build  /src/target/*/release/tuliprox /opt/tuliprox/bin/tuliprox
COPY --from=trunk-build /src/frontend/dist             /opt/tuliprox/web/dist
COPY --from=resources   /src/resources                 /opt/tuliprox/resources

# PATH convenience symlink
RUN ln -s /opt/tuliprox/bin/tuliprox /usr/local/bin/tuliprox

# Land in /opt/tuliprox/data on attach
WORKDIR /opt/tuliprox/data

EXPOSE 8901
ENTRYPOINT ["/opt/tuliprox/bin/tuliprox"]
CMD ["-s", "-p", "/opt/tuliprox/data"]

# -----------------------------------------------------------------
# Final Image #3: Debugging Environment (Alpine-based)
# -----------------------------------------------------------------
# Allow overriding the rust image tag used for debug (e.g. "1.90-alpine3.20").
FROM rust:${RUST_ALPINE_TAG} AS debug

# For Alpine, the correct target architecture typically uses 'musl'
ARG RUST_TARGET=x86_64-unknown-linux-musl
ARG TZ=UTC

# Make the build-time arguments available as run-time env vars
ENV RUST_TARGET=${RUST_TARGET}
ENV TZ=${TZ}

# Install debugging/tooling. No OpenSSL needed (app uses rustls).
# tzdata + ca-certificates for proper time/TLS behavior.
RUN apk add --no-cache \
      bash \
      build-base \
      lldb \
      gdb \
      curl \
      tzdata \
      ca-certificates \
 && update-ca-certificates || true

# Ensure the requested Rust target is available (musl for scratch-like behavior)
RUN rustup target add "${RUST_TARGET}"

# Create production-like layout under /opt (matches our final images)
WORKDIR /opt/tuliprox
COPY --from=trunk-build /src/frontend/dist ./web/dist
COPY --from=resources   /src/resources     ./resources

# Keep full source tree for debugging in /usr/src/tuliprox
WORKDIR /usr/src/tuliprox
COPY . .

# Entrypoint helper (kept from original workflow)
COPY ./docker/debug/debug-entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
# The CMD will be passed as arguments to the entrypoint script.
CMD ["tail", "-f", "/dev/null"]
