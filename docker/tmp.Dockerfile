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
ARG ALPINE_VERSION=3.22.1
ARG RUST_ALPINE_TAG=alpine
ARG DEFAULT_TZ=UTC

# =============================================================================
# Stage 0: chef  (cargo-chef prepare)
#  - Generates a recipe.json that represents all Rust dependencies
#  - Computes /rust-target from TARGETPLATFORM if RUST_TARGET not set
# =============================================================================
FROM ${GHCR_NS}/rust-wasm-tools:${BUILDPLATFORM_TAG} AS chef
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
WORKDIR /src

# Map TARGETPLATFORM -> musl target triple if RUST_TARGET not provided
RUN set -eux; \
    if [ -z "${RUST_TARGET:-}" ]; then \
      case "${TARGETPLATFORM}" in \
        "linux/amd64") echo x86_64-unknown-linux-musl  > /rust-target ;; \
        "linux/arm64") echo aarch64-unknown-linux-musl > /rust-target ;; \
        *) echo "Unsupported TARGETPLATFORM: ${TARGETPLATFORM}" >&2; exit 1 ;; \
      esac; \
    else \
      echo "${RUST_TARGET}" > /rust-target; \
    fi; \
    printf "Using RUST_TARGET=%s\n" "$(cat /rust-target)"

# Ensure the target is available in the toolchain (prebuild already has rustup)
RUN rustup target add "$(cat /rust-target)" || true

# Prepare dependency recipe (reacts to Cargo.toml/Cargo.lock changes)
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# =============================================================================
# Stage 1: deps (cargo-chef cook)
#  - Builds ONLY dependencies as cacheable Docker layers
#  - No cache-mounts here → proper cross-run caching via buildx
# =============================================================================
FROM ${GHCR_NS}/rust-wasm-tools:${BUILDPLATFORM_TAG} AS deps

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
FROM ${GHCR_NS}/rust-wasm-tools:${BUILDPLATFORM_TAG} AS rust-build

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

# =============================================================================
# Stage 3: trunk-build (WASM frontend)
#  - Build frontend with Trunk, avoiding full workspace load
#  - Creates synthetic minimal workspace (frontend + shared) only
# =============================================================================
FROM ${GHCR_NS}/rust-wasm-tools:${BUILDPLATFORM_TAG} AS trunk-build
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
WORKDIR /src

# Synthetic minimal workspace to avoid loading 'backend' in this stage
RUN set -eux; printf '%s\n' \
  '[workspace]' \
  'members=["frontend","shared"]' \
  'resolver="2"' \
  '' \
  '[workspace.package]' \
  'edition="2021"' > /src/Cargo.toml

# Manifests for wasm build
COPY frontend/Cargo.toml ./frontend/
COPY shared/Cargo.toml   ./shared/

# Minimal sources so Cargo can parse the workspace
RUN set -eux; \
  mkdir -p frontend/src shared/src && \
  echo 'fn main() {}'      > frontend/src/main.rs && \
  echo 'pub fn dummy() {}' > shared/src/lib.rs

# (Optional) Warm wasm deps; use cache-mounts with locking to avoid races
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-trunk,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,     id=cargo-git-trunk,     sharing=locked \
    set -eux; \
    cargo build --manifest-path frontend/Cargo.toml --target wasm32-unknown-unknown --release || true

# Build actual frontend
COPY frontend ./frontend
COPY shared   ./shared
WORKDIR /src/frontend
RUN set -eux; trunk build --release
# dist -> /src/frontend/dist

# =============================================================================
# Stage 4: runner (distroless, nonroot)
# =============================================================================
FROM gcr.io/distroless/static-debian12:nonroot AS runner
WORKDIR /app

# Backend binary
COPY --from=rust-build /src/target/$(cat /rust-target)/release/tuliprox /app/tuliprox
# Frontend assets (optional if your app serves them)
COPY --from=trunk-build /src/frontend/dist /app/dist

ENV RUST_LOG=info
EXPOSE 8080
USER nonroot:nonroot
ENTRYPOINT ["/app/tuliprox"]
