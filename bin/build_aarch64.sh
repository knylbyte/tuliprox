#!/usr/bin/env bash
echo "building binary for aarch64"
env RUSTFLAGS="--remap-path-prefix $HOME=~" cross build -p tuliprox --release --target aarch64-unknown-linux-musl
