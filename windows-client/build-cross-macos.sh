#!/bin/bash
set -euo pipefail
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
# Rust stable + x86_64-pc-windows-gnu std, and Zig 0.13+ are build-machine
# dependencies only. Neither is needed by end users of the resulting EXE.
: "${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:?Set this to a Zig cc wrapper targeting x86_64-windows-gnu}"
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static"
cargo build --locked --release --target x86_64-pc-windows-gnu --manifest-path "$HERE/Cargo.toml"
printf 'EXE: %s/target/x86_64-pc-windows-gnu/release/jpeg-clipboard-windows.exe\n' "$HERE"
