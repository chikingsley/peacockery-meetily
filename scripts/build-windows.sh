#!/usr/bin/env bash
# Cross-compile the Windows NSIS installer on Linux (GMK).
#
# Needs: rustup target x86_64-pc-windows-msvc, cargo-xwin, pnpm, uv, clang/lld,
# libayatana-appindicator (the Tauri CLI panics without it).
#
# Tauri's NSIS plugins are x86 Windows DLLs. Arch's `nsis` package only targets amd64 and
# cannot load them, so this script keeps its own NSIS 3.11: Tauri's data zip plus a Linux
# `makensis` built from the matching source. Both downloads are pinned by SHA-256.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
nsis_root="${NSIS_CACHE:-$HOME/.cache/nsis-win}"
nsis="$nsis_root/nsis-3.11"
zip_url="https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip"
zip_sha="c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1"
src_url="https://downloads.sourceforge.net/project/nsis/NSIS%203/3.11/nsis-3.11-src.tar.bz2"
src_sha="19e72062676ebdc67c11dc032ba80b979cdbffd3886c60b04bb442cdd401ff4b"

fetch() { # url sha dest
  [ -f "$3" ] || curl -fsSL "$1" -o "$3"
  echo "$2  $3" | sha256sum -c --quiet -
}

if [ ! -x "$nsis/Bin/makensis" ]; then
  mkdir -p "$nsis_root"
  fetch "$zip_url" "$zip_sha" "$nsis_root/nsis.zip"
  fetch "$src_url" "$src_sha" "$nsis_root/nsis-src.tar.bz2"
  [ -d "$nsis" ] || unzip -q "$nsis_root/nsis.zip" -d "$nsis_root"
  rm -rf "$nsis_root/nsis-3.11-src"
  tar -xjf "$nsis_root/nsis-src.tar.bz2" -C "$nsis_root"
  # makensis looks for Stubs/Plugins/Include next to its own Bin directory, so it must
  # live in nsis-3.11/Bin. Keep the default NSIS_MAX_STRLEN so it matches Tauri's stubs.
  (cd "$nsis_root/nsis-3.11-src" && uvx scons SKIPSTUBS=all SKIPPLUGINS=all SKIPUTILS=all \
    SKIPMISC=all NSIS_CONFIG_CONST_DATA_PATH=no makensis)
  install -m 755 "$nsis_root/nsis-3.11-src/build/urelease/makensis/makensis" "$nsis/Bin/makensis"
fi

export PATH="$nsis/Bin:$HOME/.cargo/bin:$PATH"
cd "$repo/frontend"
pnpm install --frozen-lockfile
pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --no-sign --ci

ls -1 "$repo"/target/x86_64-pc-windows-msvc/release/bundle/nsis/*-setup.exe
