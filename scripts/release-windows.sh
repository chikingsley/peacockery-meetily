#!/usr/bin/env bash
# Publish a tested, signed release to the feed used by installed clients.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
[ -z "$(git status --porcelain)" ] || { echo 'Commit the release changes first.'; exit 1; }
version="$(node -p 'require("./frontend/src-tauri/tauri.conf.json").version')"
export TAURI_SIGNING_PRIVATE_KEY="${TAURI_SIGNING_PRIVATE_KEY:-$HOME/.local/share/peacockery-meetily/updater.key}"
[ -f "$TAURI_SIGNING_PRIVATE_KEY" ] || { echo 'Provide the established update signing key.'; exit 1; }
node --test services/activation/worker.test.mjs
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xwin clippy --workspace --target x86_64-pc-windows-msvc -- -D warnings
cargo test --workspace
(cd frontend && pnpm check)
./scripts/build-windows.sh
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
installer="$repo/target/x86_64-pc-windows-msvc/release/bundle/nsis/Peacockery Meetily_${version}_x64-setup.exe"
cp "$installer" "$staging/Peacockery-Meetily-Setup.exe"
cp "$installer.sig" "$staging/Peacockery-Meetily-Setup.exe.sig"
node scripts/update-manifest.mjs "$version" "$staging/Peacockery-Meetily-Setup.exe.sig" "$staging/latest.json"
(cd "$staging" && sha256sum Peacockery-Meetily-Setup.exe > SHA256SUMS)
git push origin HEAD:main
gh release create "v$version" --repo chikingsley/peacockery-meetily --target "$(git rev-parse HEAD)" --title "Peacockery Meetily $version" --notes-file RELEASE-NOTES.md --draft
gh release upload "v$version" --repo chikingsley/peacockery-meetily "$staging/Peacockery-Meetily-Setup.exe" "$staging/Peacockery-Meetily-Setup.exe.sig" "$staging/latest.json" "$staging/SHA256SUMS"
gh release edit "v$version" --repo chikingsley/peacockery-meetily --draft=false --latest
