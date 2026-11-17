# Distribution and updates

Share the [Windows installer](https://github.com/chikingsley/peacockery-meetily/releases/latest/download/Peacockery-Meetily-Setup.exe) and the recipient's eight-digit activation code. A fresh installation opens the code entry prompt. Settings > Voice > Access also accepts the code. Successful activation stores personal Voice access in Windows Credential Manager. The installer contains the application; credentials, calendars, recordings and vocabulary remain per user.

A code lasts seven days and belongs to one installation after redemption. The same installation can retry after a dropped response. Updates retain its installation identity. The service limits redemption attempts to ten per IP address per fifteen minutes. A code is a private invitation: give it to the intended recipient.

## Issuing access

Run from the checkout on GMK:

```sh
node scripts/access.mjs invite Jerry
node scripts/access.mjs status Jerry
node scripts/access.mjs invite "Full Name" --email verified@cicahuntek.us
node scripts/access.mjs invite Jerry --new-device
node scripts/access.mjs revoke Jerry
```

The single command resolves an active CICA contact through the communications archive, reads its source record, creates or reuses the person's Voice user, mints scoped access and returns the installer and eight-digit code. Names with multiple matching addresses require `--email`. External recipients require their verified archived address with `--email`. `lookup` performs identity verification alone. `--json` returns structured results for agents. Preserve the person's identity from the user's request; ask only to resolve genuinely ambiguous matches.

Repeated invitations reuse the current code and credential, including installation-bound retries. Expired codes renew against the saved credential. `--new-device` issues independent access for another computer; `revoke` disables every credential tracked for this person by this operator. An activated code is identified explicitly. Invocation by an agent supplies the invitation to Simon; sending it to someone follows Simon's message authorization.

Operator state lives in owner-only files under `~/.local/share/peacockery-meetily/invitations/`. Jerry's original invitation and key were adopted without replacing them. The command uses the existing Cloudflare Wrangler login, the activation administrator key, and `PEACOCKERY_VOICE_ADMIN_API_KEY` from the environment or the sibling Voice checkout's private `.dev.vars`. Secrets stay in the operator's store. A per-person lock prevents concurrent issuance. An interrupted operation leaves its PID in the lock: verify that process has exited before removing its lock and retrying.

The deployed activation service is `https://meetily-activation.cheez2012.workers.dev`. Its administrator secret lives in the operator's private `~/.local/share/peacockery-meetily/activation-admin.key` and Cloudflare's `ADMIN_KEY` secret. Each grant is stored in a Durable Object addressed by the code's SHA-256 hash. Redemption atomically claims the grant for the installation UUID. The underlying Voice credential remains independently revocable.

To cancel an invitation, send an administrator-authenticated POST to `/admin/revoke` with JSON `{ "code": "<code>" }`. To revoke already activated access, delete its Voice credential through `DELETE /v1/admin/api-keys/{credential_id}`. Use the private receipt's ID. Issue a fresh code when installing on another PC.

Run `node --test scripts/access.test.mjs services/activation/worker.test.mjs` for the activation service checks. Deploy service changes with `pnpm dlx wrangler@4.157.0 deploy --config services/activation/wrangler.jsonc` using the operator's Cloudflare login. The service and the desktop release have separate deployments.

## Releasing

1. Update the version in frontend/package.json, frontend/src-tauri/Cargo.toml and tauri.conf.json. Refresh Cargo.lock. Edit RELEASE-NOTES.md and commit.
2. Run `scripts/release-windows.sh`. It checks the project, builds the signed Windows installer, pushes main, stages a GitHub draft with installer, signature, checksum and latest.json, then publishes it.
3. Open App updates on an older installation. Verify download, installation, restart and retained recordings/settings.

The established signing key lives in the operator's private `~/.local/share/peacockery-meetily/updater.key`, or a path supplied through TAURI_SIGNING_PRIVATE_KEY. Keep a secure backup. The public key is embedded in the application. Every release uses the same private key. These update signatures are separate from Windows Authenticode publisher signing; the installer currently remains unsigned from Windows' publisher perspective.

A startup notice links to Settings > General > App updates. Check for updates and Install and restart show progress and errors. The Windows installer shows progress and relaunches the app. Downloads and signatures are verified before installation. Recording, final transcription and summary jobs defer installation until they finish.

## Outlook calendars

Settings > General > Outlook calendar opens a dedicated Edge window at CICA's tenant-specific Outlook entry point for the person's own work sign-in. The app verifies the account, displays its calendar list and lets the person select a calendar. Disconnect removes the protected session and the dedicated browser profile. Calendar access is optional and separate from Voice activation. The dedicated profile starts its own session; Microsoft can require the user’s password, MFA or device compliance. Meetily observes the bearer authorization header sent by Outlook Web in that profile, validates its account and audience, verifies calendar access, then closes the window. Credentials from unrelated browser profiles and Windows account stores remain outside this flow. The app requests calendar data with GET, while the captured Outlook Web credential carries Outlook’s broader permissions.

This uses the same Outlook Web session and Outlook REST calendar route as the existing CICA collector. The November 13 Graph consent experiment belongs to a separate integration; it is unrelated to this connection route. Each coworker signs into their own Microsoft account and completes any Microsoft authentication challenge.

The session stays on that PC, encrypted with Windows DPAPI for that Windows user. Routine token renewal opens the dedicated profile headlessly. An expired browser session or provider challenge produces a Reconnect Outlook prompt. Failed renewal backs off for five minutes. Microsoft controls the web session and endpoint behavior, so provider changes can require maintenance.

Calendar reads include pagination, UTC event times, organizer and invitees. Cancelled and all-day entries are excluded from the nearby meeting list. Read-only calendar context can set a meeting's title and recording mode. Existing installations retain their private collector snapshot as the default source until a local account is connected. Disconnect disables calendar use explicitly. Existing recordings and saved meeting context remain intact.

## Automatic meeting publication

New recordings show **Publish to CICA meetings**, enabled by default. Turn it off before starting a private recording. Settings > General changes the default for future meetings. Existing recordings and imported files retain their existing publication state; the migration creates an empty delivery queue.

After a selected recording stops, the app uploads its original `audio.mp4` in 10 MiB parts to the existing CICA website queue. The website's established sync produces the shared transcript and generated notes and publishes playable audio. Local edited notes, speaker renames and later transcript edits remain local in this version; the website generates its own artifacts from the audio. The site is available to anyone with its address, as the recording switch explains.

SQLite retains the upload ID, completed parts and progress across restarts. Failed requests retry with a one-minute backoff, then offer **Retry upload** after five attempts. Lost completion receipts are reconciled against the site's pending and published lists before sending more data. The meeting page distinguishes Queued, Uploading, Website processing, Published and failure states and opens the website in the default browser. Updates wait while an upload request is active. Future uploads resume on the next launch.

## Outlook connection boundaries

The current connection targets CICA's Outlook Web tenant on Windows with Edge installed. Edge policies that block debugging produce an actionable message. Microsoft sign-in, MFA, device policy and session expiry remain provider-controlled. Automatic renewal works while the saved browser session is valid; a new challenge prompts Reconnect Outlook. The Outlook REST v2 route is a legacy provider dependency, so repeatability is verified per supported environment rather than guaranteed across Microsoft tenants. Broader distribution should evaluate Microsoft-supported calendar-specific authorization.

## Prior update verification

The 0.6.7 release was installed through the signed update flow from 0.6.6 on CICA. All 28 recording files matched their backup hashes and Voice settings were byte-identical. A second check reported the installation current.

The 0.6.9 release was installed through the published GitHub feed from 0.6.8 on CICA. Install and restart completed, all 28 recording files matched the pre-onboarding hashes, and Voice settings stayed byte-identical. Native activation retained the existing Voice credential. The app connected the CICA Outlook account, listed its four calendars, loaded the day's meetings, and filled a selected meeting's title. A forced expiry of the protected app token triggered automatic capture from the saved browser session and restored valid calendar access. Initial authentication used the existing CICA browser session after Microsoft rejected the saved password.

Release checks passed: 151 Rust tests including the documentation test, 67 frontend tests, the frontend lint/type/build gates, three activation service tests, and the Windows DPAPI large-session roundtrip and corruption checks. Live provider tests remain a separate opt-in suite.
