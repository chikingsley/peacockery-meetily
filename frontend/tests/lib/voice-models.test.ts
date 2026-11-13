import { describe, expect, test } from "bun:test";

import {
  LIVE_SPEAKER_LABEL_MODEL,
  formatKeyterms,
  languageLabel,
  liveSpeakerLabelsHelp,
  modelChoices,
  modelLabel,
  modelOptionText,
  parseKeyterms,
  supportsLiveSpeakerLabels,
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

describe("model options", () => {
  const live = [
    option(LIVE_SPEAKER_LABEL_MODEL, { label: "Voxtral Mini Realtime 3" }),
    option("deepgram-nova-3", { available: false }),
  ];

  test("labels known ids and falls back to the id", () => {
    expect(modelLabel(live, LIVE_SPEAKER_LABEL_MODEL)).toBe(
      "Voxtral Mini Realtime 3"
    );
    expect(modelLabel(live, "unknown-model")).toBe("unknown-model");
    expect(modelLabel(undefined, "x")).toBe("x");
    expect(modelLabel(live, null)).toBeNull();
  });

  test("keeps a saved choice that the server list does not have, as unavailable", () => {
    expect(modelChoices(live, "deepgram-nova-3")).toBe(live);
    const choices = modelChoices(live, "retired-model");
    expect(choices.map((c) => c.id)).toEqual([
      "retired-model",
      LIVE_SPEAKER_LABEL_MODEL,
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

  test("only Voxtral realtime 3 offers live speaker labels", () => {
    expect(supportsLiveSpeakerLabels(LIVE_SPEAKER_LABEL_MODEL)).toBe(true);
    expect(supportsLiveSpeakerLabels("deepgram-nova-3")).toBe(false);
    expect(supportsLiveSpeakerLabels("elevenlabs-scribe_v2")).toBe(false);
  });

  test("explains live speaker labels per mode", () => {
    expect(liveSpeakerLabelsHelp("split", "Them")).toContain('"Them 1"');
    expect(liveSpeakerLabelsHelp("mixed", "Them")).toContain('"Speaker 1"');
  });

  test("names languages", () => {
    expect(languageLabel("en")).toBe("English");
    expect(languageLabel("auto")).toBe("Detect automatically");
    expect(languageLabel("sw")).toBe("sw");
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
