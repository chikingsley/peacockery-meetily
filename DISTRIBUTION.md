# Distribution and updates

Share the public Windows installer at https://github.com/chikingsley/peacockery-meetily/releases/latest/download/Peacockery-Meetily-Setup.exe and deliver each recipient's private `.peacockery-access` file separately. In Settings > Voice, choose Activate access and select that file. Treat the file as a credential: anyone holding a copy can use it until revoked. It is a reusable per-recipient invitation; single-use, device-bound enrollment is a future extension. The installer contains the application; personal credentials, calendars and vocabulary stay separate.

For the current small rollout, an operator creates a separate Voice user and scoped credential for each person. Run `node --env-file=<private operator env> scripts/create-access.mjs '<recipient>' '<private output.peacockery-access>' '<existing Voice user id>'`. The user must already exist in Voice's users table; use their verified email when creating that identity. The private receipt holds the credential ID for revocation through `DELETE /v1/admin/api-keys/{credential_id}`. Preserve the distinct user identity so each person's server-side records have their own owner.

## Releasing

1. Make the change and update the version in frontend/package.json, frontend/src-tauri/Cargo.toml and tauri.conf.json. Refresh Cargo.lock. Edit RELEASE-NOTES.md and commit.
2. Run `scripts/release-windows.sh`. It checks the project, builds the signed Windows installer, pushes main, stages a GitHub draft with installer, signature, checksum and latest.json, then publishes it.
3. Open App updates on an older installation. Verify download, installation, restart and the retained recordings/settings.

The established signing key lives in the operator's private `~/.local/share/peacockery-meetily/updater.key`, or a path supplied through TAURI_SIGNING_PRIVATE_KEY. Keep a secure backup. The public key is embedded in the application. Every release must use the same private key. These update signatures are separate from Windows Authenticode publisher signing; the installer currently remains unsigned from Windows' publisher perspective.

The update feed is GitHub Releases. A startup notice links to Settings > General, where Check for updates and Install and restart show progress and errors. The Windows installer shows a progress window and relaunches the app. Downloads and signatures are verified before installation. A recording started during download defers installation; final transcript and summary jobs also defer installation.

## Calendars

The current calendar source is an account-specific private snapshot supplied by the GMK CICA collector. It is optional and ships separately from the app. Each coworker needs their own source; each source belongs to its authorized account.

For self-service calendar connection, the intended integration is a dedicated Microsoft Entra public-client app, browser authorization with PKCE, delegated Calendars.Read, account/calendar selection, OS-protected refresh tokens, and Disconnect. CICA's recorded November 13 delegated-consent attempt required administrator approval. Tenant approval and the coworker's own sign-in are prerequisites for that integration. The present release provides recording without calendar access while this separate integration is arranged. Calendar access is separate from Voice activation.

## Verified rollout

The first published release is 0.6.7. An installed 0.6.6 build on CICA discovered the public feed, downloaded the signed installer through the app, installed and relaunched as 0.6.7. A second check reported that the installation was current. All 28 existing recording files matched their backup hashes and Voice settings were byte-identical. Validation also passed 150 Rust tests, 67 frontend tests, lint, formatting, type checking and the production build. Per-person access was checked against the model catalog and a real text request; administrative calls returned HTTP 403.
