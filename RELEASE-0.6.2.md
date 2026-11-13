# 0.6.2

Meeting titles are optional. Summary generation names timestamp placeholders and preserves custom titles, including titles changed while generation runs.

The transcript toolbar exposes **Reprocess audio…** at every width. Its dialog offers a transcription model before uploading the saved audio. The live transcript remains available alongside the resulting final transcript.

Settings now include editable recognition corrections: one `heard phrase => correct spelling` per line. These case-insensitive whole-phrase rules apply to future live previews, saved live lines and reprocessed transcript turns. Provider artifacts retain their original text. Existing installations default to an empty correction list; personal vocabulary and approved corrections stay in the user's settings.

Live audio is streamed to Peacockery Voice during recording. The app saves recordings locally; the API stores its session artifacts. A full-recording batch upload occurs when final processing is requested or automatic final processing is enabled. External document/archive synchronization remains a separate workflow.

Validation: 146 Rust tests including the doc test, 65 frontend tests, frontend lint/format/type checks and production build, Windows installer build and native model-selector inspection. Installed Windows version: 0.6.2. Existing recording and provider-artifact hashes match the pre-upgrade backup. The corresponding API update fixes repeated vocabulary query parameters; a deployed Scribe v3 test with 50 hints returned two speaker-labeled turns and recoverable session output.
