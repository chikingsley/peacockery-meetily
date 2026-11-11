#!/usr/bin/env bash
# Copy the newest installer to CICA and install it silently (per-user, no admin).
# Run on GMK, where `ssh cica` is configured. The app is closed first if it is running;
# start it again from the Start menu.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
installer="$(ls -t "$repo"/target/x86_64-pc-windows-msvc/release/bundle/nsis/*-setup.exe | head -1)"
echo "Installing $(basename "$installer")"

scp -q "$installer" cica:peacockery-meetily-setup.exe
ssh cica 'powershell -NoProfile -Command "Get-Process peacockery-meetily -ErrorAction SilentlyContinue | Stop-Process -Force; $p = Start-Process $env:USERPROFILE\peacockery-meetily-setup.exe -ArgumentList /S -Wait -PassThru; if ($p.ExitCode -ne 0) { exit $p.ExitCode }; (Get-Item \"$env:LOCALAPPDATA\Peacockery Meetily\peacockery-meetily.exe\").VersionInfo.ProductVersion"'
