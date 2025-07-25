#!/usr/bin/env bash
env RUSTFLAGS="--remap-path-prefix $HOME=~" cross build -p tuliprox --release --target x86_64-apple-darwin
cd ./webui || exit 1
env RUSTFLAGS="--remap-path-prefix $HOME=~" trunk build --release