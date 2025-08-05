#!/usr/bin/env bash
cd ./webui && rm -rf dist && env RUSTFLAGS="--remap-path-prefix $HOME=~" trunk build --release
