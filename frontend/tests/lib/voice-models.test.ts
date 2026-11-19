import { describe, expect, test } from "bun:test";

import {
  AUTO_FINAL_CHOICES,
  formatKeyterms,
  languageLabel,
  liveTranscriptLabel,
  modelChoices,
  modelLabel,
  modelOptionText,
  parseKeyterms,
  retryModel,
} from "../../src/lib/voice-models";
import type { ModelOption } from "../../src/types/voice";

const option = (
  id: string,
  overrides: Partial<ModelOption> = {}
): ModelOption => ({
  available: true,
  id,
  label: id.toUpperCase(),
  note: "",
  recommended: false,
  supports_diarization: false,
  ...overrides,
});

const VOXTRAL = "mistral-voxtral-mini-transcribe-realtime-3";

describe("model options", () => {
  const live = [
    option(VOXTRAL, { label: "Voxtral Mini Realtime 3" }),
    option("deepgram-nova-3", { available: false }),
  ];

  test("labels known ids and falls back to the id", () => {
    expect(modelLabel(live, VOXTRAL)).toBe("Voxtral Mini Realtime 3");
    expect(modelLabel(live, "unknown-model")).toBe("unknown-model");
    expect(modelLabel(undefined, "x")).toBe("x");
    expect(modelLabel(live, null)).toBeNull();
  });

  test("keeps a saved choice that the server list does not have, as unavailable", () => {
    expect(modelChoices(live, "deepgram-nova-3")).toBe(live);
    const choices = modelChoices(live, "retired-model");
    expect(choices.map((c) => c.id)).toEqual([
      "retired-model",
      VOXTRAL,
      "deepgram-nova-3",
    ]);
    expect(choices[0].available).toBe(false);
    expect(modelChoices(live, null)).toBe(live);
  });

  test("marks recommended and unavailable models", () => {
    expect(modelOptionText(live[0])).toBe("Voxtral Mini Realtime 3");
    expect(
      modelOptionText(
        option("deepgram-nova-3", { label: "Deepgram Nova", recommended: true })
      )
    ).toBe("Deepgram Nova · recommended");
    expect(modelOptionText(live[1])).toBe("DEEPGRAM-NOVA-3 · unavailable");
  });

  test("names languages", () => {
    expect(languageLabel("en")).toBe("English");
    expect(languageLabel("auto")).toBe("Detect automatically");
    expect(languageLabel("sw")).toBe("sw");
  });
});

describe("final transcript rule", () => {
  const scribe = "elevenlabs-scribe_v3";
  const final = [option(scribe, { label: "Scribe v3" })];

  test("the live pill follows what the meeting recorded, not the current settings", () => {
    expect(
      liveTranscriptLabel({ live_complete: true, live_model: scribe }, final)
    ).toBe("Live · Scribe v3 · complete");
    expect(
      liveTranscriptLabel(
        { live_complete: true, live_model: "deepgram-nova-3" },
        final
      )
    ).toBe("Live · deepgram-nova-3 · complete");

    for (const meeting of [
      { live_complete: false, live_model: scribe },
      { live_complete: null, live_model: scribe },
      { live_complete: null, live_model: null },
    ]) {
      expect(liveTranscriptLabel(meeting, final)).toBe(
        "Live transcript (incomplete)"
      );
    }
  });

  test("retries with the run's model only while it is offered", () => {
    expect(retryModel(scribe, final)).toBe(scribe);
    expect(retryModel("elevenlabs-scribe_v2", final)).toBeNull();
    expect(retryModel(null, final)).toBeNull();
    expect(retryModel(scribe, undefined)).toBeNull();
  });

  test("offers the three automatic choices, recommended first", () => {
    expect(AUTO_FINAL_CHOICES.map((c) => c.value)).toEqual([
      "when_live_incomplete",
      "always",
      "never",
    ]);
  });
});

describe("vocabulary hints", () => {
  test("splits on commas and lines, trims, drops blanks and case-insensitive repeats", () => {
    expect(
      parseKeyterms("Cica-Huntek, Altaflo\n\n  Netask \ncica-huntek,")
    ).toEqual(["Cica-Huntek", "Altaflo", "Netask"]);
    expect(parseKeyterms("")).toEqual([]);
  });

  test("round-trips through the textarea format", () => {
    const terms = ["Simon Ejimofor", "GMK"];
    expect(parseKeyterms(formatKeyterms(terms))).toEqual(terms);
  });
});
