#!/usr/bin/env bash
env RUSTFLAGS="--remap-path-prefix $HOME=~" cargo build -p tuliprox --release --target x86_64-pc-windows-gnu
# cross build, is only working with cargo clean
# env RUSTFLAGS="--remap-path-prefix $HOME=~" cross build -p tuliprox --release --target x86_64-pc-windows-gnu

cd "./frontend" && env RUSTFLAGS="--remap-path-prefix $HOME=~" trunk build --release
