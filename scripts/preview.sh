#!/usr/bin/env bash
# Supervised static UI preview. Native recording stays in the Tauri desktop app.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
unit=peacockery-meetily-preview
case "${1:-status}" in
  up)
    test -f "$repo/frontend/out/index.html"
    mkdir -p "$HOME/.config/systemd/user"
    cat > "$HOME/.config/systemd/user/$unit.service" <<UNIT
[Unit]
Description=Peacockery Meetily UI preview
[Service]
WorkingDirectory=$repo/frontend
ExecStart=$(command -v bun) $repo/scripts/preview-server.ts
Restart=on-failure
UNIT
    systemctl --user daemon-reload
    systemctl --user start "$unit"
    curl --retry 8 --retry-connrefused --retry-delay 1 -fsS http://127.0.0.1:3119/ -o /dev/null
    cd "$repo"
    tunnel up 3119
    ;;
  down)
    cd "$repo"
    tunnel down peacockery-meetily
    systemctl --user stop "$unit"
    ;;
  status)
    systemctl --user show "$unit" -p ActiveState -p UnitFileState
    tunnel ls
    ;;
  *) echo 'Usage: scripts/preview.sh up|down|status' >&2; exit 2 ;;
esac
