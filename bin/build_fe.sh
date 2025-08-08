#!/usr/bin/env bash
cd ./frontend && rm -rf dist && env RUSTFLAGS="--remap-path-prefix $HOME=~" trunk build --release
