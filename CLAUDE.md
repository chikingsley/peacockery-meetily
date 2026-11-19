# CLAUDE.md

Peacockery Meetily is an internal Windows-only meeting and call recorder (Tauri 2, Rust, Next.js 16
static export). No local models: live transcript, final transcript and summaries all go through
the Peacockery Voice API.

**Read `ARCHITECTURE.md` before changing anything.** It is the contract between the Rust core,
the two crates and the frontend: settings shape, API facts, audio pipeline, database, every Tauri
command and event. Change it in the same edit when the contract changes.

## Layout

- `crates/peacockery-voice/`: API client (HTTP and realtime WebSocket), pure logic, replay fixtures.
- `crates/app-loopback/`: Windows process-tree loopback capture (Chrome audio).
- `frontend/src-tauri/`: the app (package `peacockery-meetily`, lib `app_lib`): audio capture,
  recording, live sessions, final transcript, SQLite, commands.
- `frontend/src/`: UI.
- `scripts/`: Windows build and CICA install.

## Commands

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xwin clippy --workspace --target x86_64-pc-windows-msvc -- -D warnings   # cargo-xwin is in ~/.cargo/bin
cd frontend && pnpm check                       # lint, format, typecheck, bun test, build
scripts/release-windows.sh                      # runs every gate above, builds, releases
scripts/install-cica.sh
```

## Rules

- Windows is the only target. The Linux code that remains is the minimum for `cargo test` on GMK.
- One engine. Do not add local models, other providers, compatibility paths or migration shims;
  when something is replaced, delete the old code and its tests.
- The API key lives only in Windows Credential Manager. It never reaches the webview, SQLite,
  logs or command output.
- Model ids come from the curated lists in `crates/peacockery-voice/src/catalog.rs`.
- Realtime audio is sent continuously, silence included; never gate it with a VAD.
- Recorded audio from real calls stays out of the repository.
- The app's log on CICA: `ssh cica`, then
  `%LOCALAPPDATA%\studio.peacockery.meetily\logs\peacockery-meetily.log`.
- An ssh session on CICA is not the desktop session: Credential Manager and audio devices only
  work from a process started in the logged-on session.
