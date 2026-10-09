#!/usr/bin/env bash
set -Eeuo pipefail

# Hardened native Linux release build. This mirrors build-release.ps1's compile-time
# path remapping and reproducibility settings, then lets Tauri create both required
# native Linux bundles. It never patches a compiled binary after the build.

repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$repo_dir"

export CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-false}"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"
export CARGO_TERM_COLOR=never
unset RUSTFLAGS

cargo_home="${CARGO_HOME:-${HOME}/.cargo}"
rustup_home="${RUSTUP_HOME:-${HOME}/.rustup}"

rustflags=(
  "--remap-path-prefix=${repo_dir}=/crafthub"
  "--remap-path-prefix=${cargo_home}=/cargo"
  "--remap-path-prefix=${rustup_home}=/rustup"
)
export CARGO_ENCODED_RUSTFLAGS="$(printf '%s\037' "${rustflags[@]}")"
CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS%$'\037'}"
export CARGO_ENCODED_RUSTFLAGS

if [[ ! -f Cargo.lock || ! -f package-lock.json ]]; then
  echo 'required lockfile missing' >&2
  exit 1
fi

cargo metadata --locked --format-version 1 >/dev/null
npx tauri build --bundles appimage,deb

appimages=(target/release/bundle/appimage/*.AppImage)
debs=(target/release/bundle/deb/*.deb)
[[ -f "${appimages[0]}" ]] || { echo 'AppImage missing' >&2; exit 1; }
[[ -f "${debs[0]}" ]] || { echo 'Debian package missing' >&2; exit 1; }
[[ "${#appimages[@]}" -eq 1 ]] || { echo 'expected exactly one AppImage' >&2; exit 1; }
[[ "${#debs[@]}" -eq 1 ]] || { echo 'expected exactly one Debian package' >&2; exit 1; }

echo "Linux release bundles: ${appimages[0]} and ${debs[0]}"
