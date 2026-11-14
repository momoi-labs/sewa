#!/usr/bin/env bash
# Builds sewa-agent for the install command into target/sewa-agent, one
# binary per target the install script knows. The controller serves this
# directory: point SEWA_AGENT_DIR at it.
#
# Needs cargo-zigbuild, zig, and the Rust targets below (rustup target add).
set -euo pipefail

out=${SEWA_AGENT_DIR:-target/sewa-agent}
targets=(
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-musl
  x86_64-apple-darwin
  aarch64-apple-darwin
)

args=()
for target in "${targets[@]}"; do
  args+=(--target "$target")
done
cargo zigbuild --release --locked -p sewa-agent "${args[@]}"
mkdir -p "$out"
for target in "${targets[@]}"; do
  cp "target/$target/release/sewa-agent" "$out/sewa-agent-$target"
done
