#!/usr/bin/env bash
env RUSTFLAGS="--remap-path-prefix $HOME=~" cross build -p tuliprox --release --target x86_64-apple-darwin
