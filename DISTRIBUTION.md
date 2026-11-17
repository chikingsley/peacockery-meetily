# Distribution and updates

Share the [Windows installer](https://github.com/chikingsley/peacockery-meetily/releases/latest/download/Peacockery-Meetily-Setup.exe) and the recipient's eight-digit activation code. A fresh installation opens the code entry prompt. Settings > Voice > Access also accepts the code. Successful activation stores personal Voice access in Windows Credential Manager. The installer contains the application; credentials, calendars, recordings and vocabulary remain per user.

A code lasts seven days and belongs to one installation after redemption. The same installation can retry after a dropped response. Updates retain its installation identity. The service limits redemption attempts to ten per IP address per fifteen minutes. A code is a private invitation: give it to the intended recipient.

## Issuing access

1. Create the recipient's separate Voice user using their verified email. Keep each person's server-side recordings under their own user identity.
2. Run `node --env-file=<private operator env> scripts/create-access.mjs '<recipient>' '<private access record>' '<existing Voice user id>'`. The record stays in the operator's private store. Its receipt contains the credential ID.
3. Run `node scripts/create-code.mjs '<private access record>' '<private code receipt>'`. Give the recipient the printed code and installer link. Operator records stay with the operator.

The deployed activation service is `https://meetily-activation.cheez2012.workers.dev`. Its administrator secret lives in the operator's private `~/.local/share/peacockery-meetily/activation-admin.key` and Cloudflare's `ADMIN_KEY` secret. Each grant is stored in a Durable Object addressed by the code's SHA-256 hash. Redemption atomically claims the grant for the installation UUID. The underlying Voice credential remains independently revocable.

To cancel an invitation, send an administrator-authenticated POST to `/admin/revoke` with JSON `{ "code": "<code>" }`. To revoke already activated access, delete its Voice credential through `DELETE /v1/admin/api-keys/{credential_id}`. Use the private receipt's ID. Issue a fresh code when installing on another PC.

Run `node --test services/activation/worker.test.mjs` for the activation service checks. Deploy service changes with `pnpm dlx wrangler@4.157.0 deploy --config services/activation/wrangler.jsonc` using the operator's Cloudflare login. The service and the desktop release have separate deployments.

## Releasing

1. Update the version in frontend/package.json, frontend/src-tauri/Cargo.toml and tauri.conf.json. Refresh Cargo.lock. Edit RELEASE-NOTES.md and commit.
2. Run `scripts/release-windows.sh`. It checks the project, builds the signed Windows installer, pushes main, stages a GitHub draft with installer, signature, checksum and latest.json, then publishes it.
3. Open App updates on an older installation. Verify download, installation, restart and retained recordings/settings.

The established signing key lives in the operator's private `~/.local/share/peacockery-meetily/updater.key`, or a path supplied through TAURI_SIGNING_PRIVATE_KEY. Keep a secure backup. The public key is embedded in the application. Every release uses the same private key. These update signatures are separate from Windows Authenticode publisher signing; the installer currently remains unsigned from Windows' publisher perspective.

A startup notice links to Settings > General > App updates. Check for updates and Install and restart show progress and errors. The Windows installer shows progress and relaunches the app. Downloads and signatures are verified before installation. Recording, final transcription and summary jobs defer installation until they finish.

## Outlook calendars

Settings > General > Outlook calendar opens a dedicated Edge window at CICA's tenant-specific Outlook entry point for the person's own work sign-in. The app verifies the account, displays its calendar list and lets the person select a calendar. Disconnect removes the protected session and the dedicated browser profile. Calendar access is optional and separate from Voice activation.

This uses the same Outlook Web session and Outlook REST calendar route as the existing CICA collector. The November 13 Graph consent experiment belongs to a separate integration; it is unrelated to this connection route. Each coworker signs into their own Microsoft account and completes any Microsoft authentication challenge.

The session stays on that PC, encrypted with Windows DPAPI for that Windows user. Routine token renewal opens the dedicated profile headlessly. An expired browser session or provider challenge produces a Reconnect Outlook prompt. Failed renewal backs off for five minutes. Microsoft controls the web session and endpoint behavior, so provider changes can require maintenance.

Calendar reads include pagination, UTC event times, organizer and invitees. Cancelled and all-day entries are excluded from the nearby meeting list. Read-only calendar context can set a meeting's title and recording mode. Existing installations retain their private collector snapshot as the default source until a local account is connected. Disconnect disables calendar use explicitly. Existing recordings and saved meeting context remain intact.

## Prior update verification

The 0.6.7 release was installed through the signed update flow from 0.6.6 on CICA. All 28 recording files matched their backup hashes and Voice settings were byte-identical. A second check reported the installation current.

The 0.6.9 release was installed through the published GitHub feed from 0.6.8 on CICA. Install and restart completed, all 28 recording files matched the pre-onboarding hashes, and Voice settings stayed byte-identical. Native activation retained the existing Voice credential. The app connected the CICA Outlook account, listed its four calendars, loaded the day's meetings, and filled a selected meeting's title. A forced expiry of the protected app token triggered automatic capture from the saved browser session and restored valid calendar access. Initial authentication used the existing CICA browser session after Microsoft rejected the saved password.

Release checks passed: 151 Rust tests including the documentation test, 67 frontend tests, the frontend lint/type/build gates, three activation service tests, and the Windows DPAPI large-session roundtrip and corruption checks. Live provider tests remain a separate opt-in suite.
