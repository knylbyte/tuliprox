# =================================================================
#
# Part 1: Prerequisite Build Stages
#
# These stages are used as building blocks for the final images.
# They prepare the Rust binary, the Node.js frontend, and other resources.
#
# =================================================================

# -----------------------------------------------------------------
# Stage 1: Build the Rust binary for production
# -----------------------------------------------------------------
FROM rust:bookworm AS rust-build

# Get target architecture
ARG RUST_TARGET

RUN apt-get update && apt-get install -y --no-install-recommends pkg-config musl-tools libssl-dev

# Update Rust toolchain and add necessary target
RUN rustup update && rustup target add $RUST_TARGET

# Set Rust compiler flags for better optimization and reproducibility
ENV RUSTFLAGS='--remap-path-prefix $HOME=~ -C target-feature=+crt-static'

# Copy only Cargo.toml and Cargo.lock for dependency caching
WORKDIR /src
COPY Cargo.toml Cargo.lock ./

# Create a dummy src/main.rs to build dependencies only
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build -p tuliprox --target $RUST_TARGET --release || true

# Now copy the actual source code and build the project
COPY . .
RUN cargo build -p tuliprox --target $RUST_TARGET --release

# -----------------------------------------------------------------
# Stage 2: Build the Node.js frontend
# -----------------------------------------------------------------
FROM node:lts AS node-build

ENV NODE_OPTIONS=--openssl-legacy-provider

WORKDIR /app

# Copy package.json files first for better caching
COPY ./frontend/package.json ./

# Install dependencies
RUN yarn install --prefer-offline --non-interactive --production=false

# Copy the rest of the frontend code
COPY ./frontend /app

# Build the frontend
RUN yarn build && rm -rf node_modules

# -----------------------------------------------------------------
# Stage 3: Build video resources with ffmpeg
# -----------------------------------------------------------------
FROM linuxserver/ffmpeg:latest AS resource-build

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

# -----------------------------------------------------------------
# Stage 4: Prepare timezone data
# -----------------------------------------------------------------
FROM alpine:latest AS tz-prep

ARG TZ=UTC
ENV TZ=${TZ}

RUN apk add --no-cache tzdata \
  && mkdir -p /output/etc \
  && mkdir -p /output/usr/share \
  && cp -r /usr/share/zoneinfo /output/usr/share/zoneinfo \
  && ln -sf /usr/share/zoneinfo/${TZ} /output/etc/localtime


# -----------------------------------------------------------------
# Stage 5: Shadowsocks client (sslocal)
# -----------------------------------------------------------------
FROM ghcr.io/shadowsocks/sslocal-rust:latest AS sslocal   # pre-built /usr/bin/sslocal


# =================================================================
#
# Part 2: Final Image Stages
#
# These stages build the final, runnable images by assembling
# the artifacts from the prerequisite stages above.
#
# =================================================================

# -----------------------------------------------------------------
# Final Image #1: Debugging Environment (Alpine-based)
# -----------------------------------------------------------------
FROM rust:alpine AS debug

# For Alpine, the correct target architecture uses 'musl'
ARG RUST_TARGET=x86_64-unknown-linux-musl
ARG TZ=UTC

# Make the build-time argument available as a run-time environment variable
ENV RUST_TARGET=${RUST_TARGET}
ENV TZ=${TZ}

# Install build dependencies and the lldb debugger for Alpine
# Added 'perl' which is required to build the native OpenSSL library.
RUN apk add --no-cache bash build-base openssl-dev lldb gdb perl

# Update Rust toolchain and add the necessary target
RUN rustup update && rustup target add $RUST_TARGET

# Create the final application layout to mimic production
WORKDIR /app
COPY --from=node-build /app/build ./web
COPY --from=resource-build /src/resources ./resources

# Create a separate directory for the source code
WORKDIR /usr/src/tuliprox
COPY . .

# Copy the entrypoint script and set it as the entrypoint for the container.
COPY ./docker/debug/debug-entrypoint.sh /usr/local/bin/entrypoint.sh
# Ensure the script is executable
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]

# The CMD will be passed as arguments to the entrypoint script.
CMD ["tail", "-f", "/dev/null"]

# -----------------------------------------------------------------
# Final Image #2: Production image based on scratch
# -----------------------------------------------------------------
FROM scratch AS scratch-final

ARG RUST_TARGET

# Copy timezone data and localtime from tz-prep
COPY --from=tz-prep /output/usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=tz-prep /output/etc/localtime /etc/localtime

# Certificates
COPY --from=rust-build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# RUN ln -sf /usr/share/zoneinfo/${TZ} /etc/localtime

# App
WORKDIR /app
COPY --from=rust-build /src/target/${RUST_TARGET}/release/tuliprox ./tuliprox
COPY --from=node-build /app/build ./web
COPY --from=resource-build /src/resources ./resources

# Shadowsocks client
COPY --from=sslocal /usr/bin/sslocal /usr/local/bin/sslocal

ENTRYPOINT ["/app/tuliprox"]
CMD ["-s", "-p", "/app/config"]

# -----------------------------------------------------------------
# Final Image #3: Production image based on Alpine
# -----------------------------------------------------------------
FROM alpine:latest AS alpine-final

ARG RUST_TARGET
ARG TZ=UTC
ENV TZ=${TZ}

RUN apk add --no-cache bash curl ca-certificates tini

# Optional Shadowsocks client
ENV SS_CLIENT_ENABLE=0

COPY --from=rust-build /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=rust-build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Default timezone fallback
RUN ln -sf /usr/share/zoneinfo/${TZ} /etc/localtime

WORKDIR /app
COPY --from=rust-build /src/target/${RUST_TARGET}/release/tuliprox ./tuliprox
COPY --from=node-build /app/build ./web
COPY --from=resource-build /src/resources ./resources

# Shadowsocks client
COPY --from=sslocal /usr/bin/sslocal /usr/local/bin/sslocal
RUN chmod +x /usr/local/bin/sslocal

# Entrypoint script to optionally start sslocal
COPY ./docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/entrypoint.sh"]
CMD ["/app/tuliprox", "-s", "-p", "/app/config"]
