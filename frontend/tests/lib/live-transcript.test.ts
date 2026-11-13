import { describe, expect, test } from "bun:test";

import {
  applyInterim,
  applyLiveStatus,
  byChannel,
  insertLiveLine,
  isFinalInProgress,
  isSystemSilent,
} from "../../src/lib/live-transcript";
import type {
  LiveLine,
  LiveTranscriptionStatus,
  TranscriptInterim,
} from "../../src/types/voice";

const line = (
  sequence_id: number,
  audio_start_time: number,
  meeting_id = "m1"
): LiveLine => ({
  audio_end_time: audio_start_time + 1,
  audio_start_time,
  channel: "mic",
  confidence: null,
  duration: 1,
  is_partial: false,
  meeting_id,
  sequence_id,
  source: "live",
  speaker: "Me",
  text: `line ${sequence_id}`,
  timestamp: "10:00:00",
});

const interim = (
  channel: TranscriptInterim["channel"],
  text: string
): TranscriptInterim => ({
  channel,
  meeting_id: "m1",
  speaker: null,
  text,
});

const status = (
  channel: LiveTranscriptionStatus["channel"],
  state: LiveTranscriptionStatus["state"]
): LiveTranscriptionStatus => ({
  channel,
  meeting_id: "m1",
  message: null,
  model: "mistral-voxtral-mini-transcribe-realtime-3",
  state,
});

describe("transcript-update reducer", () => {
  test("orders by audio time, then sequence", () => {
    let lines: LiveLine[] = [];

    for (const l of [line(0, 5), line(1, 2), line(2, 5)]) {
      lines = insertLiveLine(lines, l);
    }

    expect(lines.map((l) => l.sequence_id)).toEqual([1, 0, 2]);
  });

  test("ignores a repeated sequence id", () => {
    const lines = insertLiveLine([line(0, 1)], line(0, 1));
    expect(lines).toHaveLength(1);
  });

  test("a line of another meeting starts over", () => {
    const lines = insertLiveLine([line(0, 1), line(1, 2)], line(0, 0, "m2"));
    expect(lines.map((l) => l.meeting_id)).toEqual(["m2"]);
  });
});

describe("transcript-interim reducer", () => {
  test("replaces per channel and clears on empty text", () => {
    let interims = applyInterim({}, interim("mic", "hel"));
    interims = applyInterim(interims, interim("mic", "hello"));
    interims = applyInterim(interims, interim("system", "hi"));
    expect(byChannel(interims).map((i) => i.text)).toEqual(["hello", "hi"]);
    interims = applyInterim(interims, interim("mic", ""));
    expect(byChannel(interims).map((i) => i.channel)).toEqual(["system"]);
  });
});

describe("live-transcription-status reducer", () => {
  test("keeps the latest state per channel and drops closed sessions", () => {
    let statuses = applyLiveStatus({}, status("system", "connecting"));
    statuses = applyLiveStatus(statuses, status("mic", "live"));
    statuses = applyLiveStatus(statuses, status("system", "reconnecting"));
    expect(byChannel(statuses).map((s) => `${s.channel}:${s.state}`)).toEqual([
      "mic:live",
      "system:reconnecting",
    ]);
    statuses = applyLiveStatus(statuses, status("mic", "closed"));
    expect(byChannel(statuses).map((s) => s.channel)).toEqual(["system"]);
  });
});

describe("status helpers", () => {
  test("warns about system silence only past 30 s while actively recording", () => {
    const levels = (s: number) => ({
      mic: 0.4,
      system: 0,
      system_silent_seconds: s,
    });

    expect(isSystemSilent(levels(30), true, false)).toBe(false);
    expect(isSystemSilent(levels(31), true, false)).toBe(true);
    expect(isSystemSilent(levels(90), false, false)).toBe(false);
    expect(isSystemSilent(levels(90), true, true)).toBe(false);
  });

  test("final transcript in-progress states", () => {
    expect(
      ["queued", "uploading", "transcribing"].every((s) =>
        isFinalInProgress(s as never)
      )
    ).toBe(true);
    expect(isFinalInProgress("done")).toBe(false);
    expect(isFinalInProgress("failed")).toBe(false);
    expect(isFinalInProgress(null)).toBe(false);
  });
});
