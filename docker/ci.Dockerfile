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
ARG RUST_ALPINE_TAG=alpine
ARG DEFAULT_TZ=UTC
ARG SCCACHE_DIR=~/.cache/sccache
# ARG SCCACHE_GHA_ENABLED=off
# ARG SCCACHE_GHA_CACHE_SIZE
# ARG SCCACHE_GHA_VERSION

# =============================================================================
# Stage 0: chef  (cargo-chef prepare)
#  - Generates a recipe.json that represents all Rust dependencies
#  - Computes /rust-target from TARGETPLATFORM if RUST_TARGET not set
# =============================================================================
FROM ${GHCR_NS}/tuliprox-build-tools:${BUILDPLATFORM_TAG} AS chef

ARG TARGETPLATFORM
ARG RUST_TARGET
ARG SCCACHE_DIR
# ARG SCCACHE_GHA_ENABLED
# ARG SCCACHE_GHA_CACHE_SIZE
# ARG SCCACHE_GHA_VERSION

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV SCCACHE_DIR=${SCCACHE_DIR}
# ENV SCCACHE_GHA_ENABLED=${SCCACHE_GHA_ENABLED}
# ENV SCCACHE_GHA_CACHE_SIZE=${SCCACHE_GHA_CACHE_SIZE}
# ENV SCCACHE_GHA_VERSION=${SCCACHE_GHA_VERSION}

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

# Ensure the target is available in the toolchain (prebuild already has rustup)
RUN rustup target add "$(cat /rust-target)" || true

# =============================================================================
# Stage 2: backend-planner (cargo-chef prepare)
#  - Minimal synthetic workspace (backend + shared only) to avoid pulling in frontend
#  - Generates a recipe that describes all Rust deps for the specified target
# =============================================================================
FROM chef AS backend-planner

RUN echo "starting planner stage with sccache dir: ${SCCACHE_DIR}"

WORKDIR /src

# # Synthetic minimal workspace (backend + shared only) generated ahead of time
# COPY docker/build-tools/cargo-chef/backend/Cargo.toml ./Cargo.toml
# COPY docker/build-tools/cargo-chef/backend/Cargo.lock ./Cargo.lock

# # Copy only the manifests/build scripts required to resolve dependencies.
# This keeps the recipe layer stable when only source files change.
# COPY backend/build.rs ./backend/build.rs
# COPY backend/Cargo.toml ./backend/Cargo.toml
# COPY shared/Cargo.toml ./shared/Cargo.toml

# RUN set -eux; \
#     mkdir -p backend/src shared/src; \
#     printf 'fn main() {}\n' > backend/src/main.rs; \
#     : > shared/src/lib.rs

# # Produce the dependency recipe using the existing lockfile.
# RUN set -eux; \
#     cargo chef prepare --recipe-path backend-recipe.json

COPY . .

RUN set -eux; \
    sed -i -E '/^\s*members\s*=\s*\[/ { s/(,\s*)?"frontend"(,\s*)?/\1/g; s/\[\s*,/\[/; s/,\s*\]/]/ }' Cargo.toml

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=${SCCACHE_DIR},id=sccache-${TARGETPLATFORM},sharing=locked \
    set -eux; \
    cargo chef prepare --recipe-path backend-recipe.json

# =============================================================================
# Stage 3: backend-build (cargo-chef cook && build application code)
#  - Builds dependencies as cacheable Docker layers
#  - Builds backend binary statically (musl)
# =============================================================================
FROM chef AS backend-builder

ENV RUSTFLAGS='--remap-path-prefix=/root=~ -C target-feature=+crt-static'

WORKDIR /src

# # Recreate the minimal workspace layout using the pre-generated manifest
# COPY docker/build-tools/cargo-chef/backend/Cargo.toml ./Cargo.toml

# # Copy only the manifests/build scripts required to resolve dependencies.
# COPY backend/build.rs ./backend/build.rs
# COPY backend/Cargo.toml ./backend/Cargo.toml
# COPY shared/Cargo.toml ./shared/Cargo.toml

# RUN set -eux; \
#     mkdir -p backend/src shared/src; \
#     printf 'fn main() {}\n' > backend/src/main.rs; \
#     : > shared/src/lib.rs

# # Cook: compile only dependencies (cacheable layer)
# COPY --from=backend-planner /src/backend-recipe.json ./backend-recipe.json
# COPY --from=backend-planner /src/Cargo.lock ./Cargo.lock

COPY --from=backend-planner /src/backend-recipe.json ./backend-recipe.json

# Build dependencies - this is the caching Docker layer!
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=${SCCACHE_DIR},id=sccache-${TARGETPLATFORM},sharing=locked \
    set -eux; \
    cargo chef cook --release --locked --target "$(cat /rust-target)" --recipe-path backend-recipe.json  

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=${SCCACHE_DIR},id=sccache-${TARGETPLATFORM},sharing=locked \
    set -eux; \
    cargo build --release --target "$(cat /rust-target)" --locked --bin tuliprox

# =============================================================================
# Stage 4: frontend-planner (cargo-chef prepare for WASM)
#  - Minimal synthetic workspace (frontend + shared only) to avoid pulling in backend
#  - Generates a recipe that describes all Rust deps for the WASM target
# =============================================================================
FROM chef AS frontend-planner

WORKDIR /src

# # Synthetic minimal workspace (frontend + shared only) generated ahead of time
# COPY docker/build-tools/cargo-chef/frontend/Cargo.toml ./Cargo.toml
# COPY docker/build-tools/cargo-chef/frontend/Cargo.lock ./Cargo.lock

# # Copy only the manifests required for dependency resolution.
# COPY frontend/Cargo.toml ./frontend/Cargo.toml
# COPY shared/Cargo.toml   ./shared/Cargo.toml

# RUN set -eux; \
#     mkdir -p frontend/src shared/src; \
#     : > frontend/src/lib.rs; \
#     : > shared/src/lib.rs

COPY . .

RUN set -eux; \
    sed -i -E '/^\s*members\s*=\s*\[/ { s/(,\s*)?"backend"(,\s*)?/\1/g; s/\[\s*,/\[/; s/,\s*\]/]/ }' Cargo.toml

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=${SCCACHE_DIR},id=sccache-${TARGETPLATFORM},sharing=locked \
    set -eux; \
    cargo chef prepare --recipe-path frontend-recipe.json


# =============================================================================
# Stage 5: frontend-builder (cook + trunk build)
#  - Builds WASM dependencies as cacheable layers
#  - Builds the actual frontend with Trunk using the cached deps
# =============================================================================
FROM chef AS frontend-builder

WORKDIR /src

# # Recreate the minimal workspace layout using the pre-generated manifest
# COPY docker/build-tools/cargo-chef/frontend/Cargo.toml ./Cargo.toml

# # Copy only the manifests required for dependency resolution.
# COPY frontend/Cargo.toml ./frontend/Cargo.toml
# COPY shared/Cargo.toml   ./shared/Cargo.toml

# RUN set -eux; \
#     mkdir -p frontend/src shared/src; \
#     : > frontend/src/lib.rs; \
#     : > shared/src/lib.rs

# # Cook: compile only dependencies (cacheable layer)
# COPY --from=frontend-planner /src/frontend-recipe.json ./frontend-recipe.json
# COPY --from=frontend-planner /src/Cargo.lock ./Cargo.lock

COPY --from=frontend-planner /src/frontend-recipe.json ./frontend-recipe.json

# Build dependencies - this is the caching Docker layer!
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=${SCCACHE_DIR},id=sccache-${TARGETPLATFORM},sharing=locked \
    set -eux; \
    cargo chef cook --release --locked --target wasm32-unknown-unknown --recipe-path frontend-recipe.json

# # Keep using the planner's lockfile so the trimmed workspace stays consistent.
# COPY --from=frontend-planner /src/Cargo.lock ./Cargo.lock

# COPY frontend ./frontend
# COPY shared   ./shared

# # Build the actual frontend (Trunk will leverage the cooked deps)
# WORKDIR /src/frontend

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETPLATFORM},sharing=locked \
    --mount=type=cache,target=${SCCACHE_DIR},id=sccache-${TARGETPLATFORM},sharing=locked \
    set -eux; \
    trunk build --release

# dist -> /src/frontend/dist

# -----------------------------------------------------------------
# Stage 6: tzdata/zoneinfo supplier (shared)
# -----------------------------------------------------------------
FROM alpine:${ALPINE_VER} AS tzdata
RUN set -eux; \
    apk add --no-cache tzdata ca-certificates; \
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

# Put runtime data under /opt/tuliprox/data (default landing dir)
WORKDIR /opt/tuliprox/data

# Copy zoneinfo + CA store for TLS & timezones
COPY --from=tzdata /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=tzdata /etc/ssl/certs      /etc/ssl/certs

# Copy binary & assets into /opt tree
COPY --from=backend-builder   /src/target/*/release/tuliprox  /opt/tuliprox/bin/tuliprox
COPY --from=frontend-builder  /src/frontend/dist              /opt/tuliprox/web/dist
COPY --from=resources         /src/resources                  /opt/tuliprox/resources

# In scratch we cannot create symlinks (no shell); duplicate to PATH location
COPY --from=backend-builder   /src/target/*/release/tuliprox /usr/local/bin/tuliprox

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
    update-ca-certificates

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
