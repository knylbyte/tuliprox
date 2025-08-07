#!/usr/bin/env bash
echo "building binary for armv7"
env RUSTFLAGS="--remap-path-prefix $HOME=~" cross build -p tuliprox --release --target armv7-unknown-linux-musleabihf
cd ./frontend || exit 1
env RUSTFLAGS="--remap-path-prefix $HOME=~" trunk build --release