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
# - sccache for Rust compilation caching -> https://crates.io/crates/sccache/0.3.3
#
# =================================================================

# -----------------------------------------------------------------
# Global configuration (override with --build-arg ...)
# -----------------------------------------------------------------
ARG GHCR_NS=ghcr.io/euzu/tuliprox
ARG BUILDPLATFORM_TAG=latest
ARG ALPINE_VER=3.22.2
ARG DEBUG_ALPINE_TAG=alpine
ARG DEFAULT_TZ=UTC
ARG SCCACHE_LOG=info
ARG MOLD_ENABLED=false
# ARG SCCACHE_GHA_ENABLED=off
# ARG SCCACHE_GHA_CACHE_SIZE
# ARG SCCACHE_GHA_VERSION

# =============================================================================
# Stage 0: chef  (cargo-chef prepare)
#  - Generates a recipe.json that represents all Rust dependencies
#  - Computes /rust-target from TARGETPLATFORM if RUST_TARGET not set
# =============================================================================
FROM ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG} AS chef

SHELL ["/bin/bash", "-e", "-u", "-x", "-o", "pipefail", "-c"]

ARG TARGETPLATFORM
ARG BUILDPLATFORM_TAG
ARG RUST_TARGET
ARG CARGO_HOME
ARG SCCACHE_LOG
ARG MOLD_ENABLED
# ARG SCCACHE_GHA_ENABLED
# ARG SCCACHE_GHA_CACHE_SIZE
# ARG SCCACHE_GHA_VERSION

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV CARGO_HOME=${CARGO_HOME}
ENV CARGO_TARGET_DIR=${CARGO_HOME}/target
ENV SCCACHE_DIR=${CARGO_HOME}/sccache
ENV SCCACHE_LOG=${SCCACHE_LOG}
# ENV SCCACHE_GHA_ENABLED=${SCCACHE_GHA_ENABLED}
# ENV SCCACHE_GHA_CACHE_SIZE=${SCCACHE_GHA_CACHE_SIZE}
# ENV SCCACHE_GHA_VERSION=${SCCACHE_GHA_VERSION}

ENV RUSTFLAGS='--remap-path-prefix=/root=~ -C target-feature=+crt-static'
ENV RUST_BACKTRACE=1

# Map TARGETPLATFORM -> RUST_TARGET (musl for scratch)
# - amd64  -> x86_64-unknown-linux-musl
# - arm64  -> aarch64-unknown-linux-musl
# - arm/v7 -> armv7-unknown-linux-musleabihf
RUN \
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

# Ensure the target is available in the toolchain (prebuild already has rustup)
RUN rustup target add "$(cat /rust-target)" || true

RUN if [ "${MOLD_ENABLED}" = "true" ]; then \
      export RUSTFLAGS="${RUSTFLAGS} -C link-arg=-fuse-ld=mold"; \
    fi

FROM chef AS cache-import

# see also: https://doc.rust-lang.org/cargo/guide/cargo-home.html#caching-the-cargo-home-in-ci
# because of our tuliprox-build-tools base image, we can't cache the $CARGO_HOME/bin directory

RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=bind,from=ctx_cache,target=/cache,readonly \
    ls -al  /cache; \
    if [[ -s /cache/cargo-registry-index.tar ]]; then \
    tar -C "${CARGO_HOME}/registry" -xf /cache/cargo-registry-index.tar; \
    fi; \
    if [[ -s /cache/cargo-registry-cache.tar ]]; then \
    tar -C "${CARGO_HOME}/registry" -xf /cache/cargo-registry-cache.tar; \
    fi; \
    if [[ -s /cache/cargo-git-db.tar ]]; then \
    tar -C "${CARGO_HOME}/git" -xf /cache/cargo-git-db.tar; \
    fi; \
    if [[ -s /cache/cargo-target.tar ]]; then \
    tar -C "${CARGO_HOME}" -xf /cache/cargo-target.tar; \
    fi; \
    if [[ -s /cache/sccache.tar ]]; then \
      tar -C "${CARGO_HOME}" -xf /cache/sccache.tar; \
    fi;

RUN echo ok > /.build-cache-import

# =============================================================================
# Stage 2: backend-planner (cargo-chef prepare)
#  - Minimal synthetic workspace (backend + shared only) to avoid pulling in frontend
#  - Generates a recipe that describes all Rust deps for the specified target
# =============================================================================
FROM chef AS backend-planner

COPY --from=cache-import  /.build-cache-import  /.build-cache-import

WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY backend ./backend
COPY frontend ./frontend
COPY shared ./shared

RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    cargo chef prepare --recipe-path backend-recipe.json

# =============================================================================
# Stage 3: backend-build (cargo-chef cook && build application code)
#  - Builds dependencies as cacheable Docker layers
#  - Builds backend binary statically (musl)
# =============================================================================
FROM chef AS backend-builder

WORKDIR /src
COPY --from=backend-planner /src/backend-recipe.json ./backend-recipe.json

# Build dependencies - this is the caching Docker layer!
RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    cargo chef cook --release --target "$(cat /rust-target)" --recipe-path backend-recipe.json; \
    sccache -s

COPY Cargo.toml Cargo.lock ./
COPY backend  ./backend
COPY frontend ./frontend
COPY shared   ./shared

# Build the actual backend binary
RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    TARGET="$(cat /rust-target)"; \
    cargo build --release --target "${TARGET}" --bin tuliprox; \
    sccache -s; \
    mkdir -p ./target/${TARGET}/release; \
    mv ${CARGO_TARGET_DIR}/${TARGET}/release/tuliprox ./target/${TARGET}/release/tuliprox

# =============================================================================
# Stage 4: frontend-planner (cargo-chef prepare for WASM)
#  - Minimal synthetic workspace (frontend + shared only) to avoid pulling in backend
#  - Generates a recipe that describes all Rust deps for the WASM target
# =============================================================================
FROM chef AS frontend-planner

COPY --from=cache-import  /.build-cache-import  /.build-cache-import

WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY frontend ./frontend
COPY shared   ./shared

RUN sed -i 's/members = \["backend", "frontend", "shared"\]/members = ["frontend", "shared"]/' Cargo.toml

RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    cargo chef prepare --recipe-path frontend-recipe.json

# =============================================================================
# Stage 5: frontend-builder (cook + trunk build)
#  - Builds WASM dependencies as cacheable layers
#  - Builds the actual frontend with Trunk using the cached deps
# =============================================================================
FROM chef AS frontend-builder

WORKDIR /src

COPY --from=frontend-planner /src/frontend-recipe.json ./frontend-recipe.json
COPY Cargo.toml Cargo.lock ./
COPY frontend ./frontend
COPY shared ./shared

RUN sed -i 's/members = \["backend", "frontend", "shared"\]/members = ["frontend", "shared"]/' Cargo.toml

# Build dependencies - this is the caching Docker layer!
RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    cargo chef cook --release --target wasm32-unknown-unknown --recipe-path frontend-recipe.json; \
    sccache -s

COPY . .

# Build the actual frontend with Trunk
RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    mkdir -p ./frontend/dist; \
    trunk build --release --config ./frontend/Trunk.toml --dist ./frontend/dist; \
    sccache -s

# -----------------------------------------------------------------
# Stage 6: tzdata/zoneinfo supplier (shared)
# -----------------------------------------------------------------
FROM alpine:${ALPINE_VER} AS tzdata
RUN apk add --no-cache tzdata ca-certificates; \
    update-ca-certificates; \
    test -d /usr/share/zoneinfo

# -----------------------------------------------------------------
# Stage 7: Resources (prebuilt ffmpeg outputs)
# -----------------------------------------------------------------
FROM ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG} AS resources
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

# Copy zoneinfo + CA store for TLS & timezones
COPY --from=tzdata /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=tzdata /etc/ssl/certs      /etc/ssl/certs

# Copy binary & assets into /opt tree
COPY --from=backend-builder   /src/target/*/release/tuliprox  /opt/tuliprox/bin/tuliprox
COPY --from=frontend-builder  /src/frontend/dist              /opt/tuliprox/web/dist
COPY --from=resources         /src/resources                  /opt/tuliprox/resources

# In scratch we cannot create symlinks (no shell); duplicate to PATH location
COPY --from=backend-builder   /src/target/*/release/tuliprox /usr/local/bin/tuliprox

# Put runtime data under /opt/tuliprox (default landing dir)
WORKDIR /opt/tuliprox

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
RUN apk add --no-cache ca-certificates bash curl tshark; \
    update-ca-certificates

# Copy zoneinfo & CA store
COPY --from=tzdata /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=tzdata /etc/ssl/certs      /etc/ssl/certs

# Copy binary & assets into /opt tree
COPY --from=backend-builder   /src/target/*/release/tuliprox  /opt/tuliprox/bin/tuliprox
COPY --from=frontend-builder  /src/frontend/dist              /opt/tuliprox/web/dist
COPY --from=resources         /src/resources                  /opt/tuliprox/resources

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
FROM rust:${DEBUG_ALPINE_TAG} AS debug

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
COPY --from=frontend-builder  /src/frontend/dist             /opt/tuliprox/web/dist
COPY --from=resources         /src/resources                 /opt/tuliprox/resources

# Keep full source tree for debugging in /usr/src/tuliprox
WORKDIR /usr/src/tuliprox
COPY . .

# Entrypoint helper (kept from original workflow)
COPY ./docker/debug/debug-entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
# The CMD will be passed as arguments to the entrypoint script.
CMD ["tail", "-f", "/dev/null"]

# =================================================================
#
# Part 3: Build cache export image
#
# These stages build the cachable images for ci platforms
# like github actions.
#
# =================================================================

# -----------------------------------------------------------------
# Backup Image #1: Cache TAR Builder (FROM chef) -> build all TARs witch cache directories
# -----------------------------------------------------------------
FROM chef AS cache-pack

# --- COPY all relevant files from build stages to enable the best possible layer caching ---

# Copy zoneinfo + CA store for TLS & timezones
COPY --from=tzdata /usr/share/zoneinfo /tmp/tuliprox/usr/share/zoneinfo
COPY --from=tzdata /etc/ssl/certs      /tmp/tuliprox/etc/ssl/certs

# Copy binary & assets into /opt tree
COPY --from=backend-builder   /src/target/*/release/tuliprox  /tmp/tuliprox/opt/tuliprox/bin/tuliprox
COPY --from=frontend-builder  /src/frontend/dist              /tmp/tuliprox/opt/tuliprox/web/dist
COPY --from=resources         /src/resources                  /tmp/tuliprox/opt/tuliprox/resources

RUN --mount=type=cache,target=${CARGO_HOME}/registry/index,id=cargo-registry-index-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/registry/cache,id=cargo-registry-cache-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git/db,id=cargo-git-db-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/target,id=cargo-target-${BUILDPLATFORM_TAG},sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/sccache,id=sccache-${BUILDPLATFORM_TAG},sharing=locked \
    mkdir -p /out; \
    tar -C ${CARGO_HOME} -cf /out/cargo-registry-index.tar registry/index   || true; \
    tar -C ${CARGO_HOME} -cf /out/cargo-registry-cache.tar registry/cache   || true; \
    tar -C ${CARGO_HOME} -cf /out/cargo-git-db.tar         git/db           || true; \
    tar -C ${CARGO_HOME} -cf /out/cargo-target.tar         target           || true; \
    tar -C ${CARGO_HOME} -cf /out/sccache.tar              sccache          || true

# -----------------------------------------------------------------
# Final Image #1: Cache Exporter (from scratch) -> for CI caching
# -----------------------------------------------------------------
FROM scratch AS cache-export

COPY --from=cache-pack /out/ /out/
