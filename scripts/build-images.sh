#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo +stable build --release --locked
mkdir -p build
arm-none-eabi-objcopy -O binary target/thumbv6m-none-eabi/release/bonanza-bridge-fw build/bonanza-bridge-fw-1004x.bin
elf2uf2-rs target/thumbv6m-none-eabi/release/bonanza-bridge-fw build/bonanza-bridge-fw-1004x.uf2
