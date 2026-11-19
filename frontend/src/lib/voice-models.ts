import type { AutoFinal, MeetingMetadata, ModelOption } from "@/types/voice";

export const DEFAULT_BASE_URL = "https://voice-lab.peacockery.studio";

export const LANGUAGES: { code: string; label: string }[] = [
  { code: "en", label: "English" },
  { code: "auto", label: "Detect automatically" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "pt", label: "Portuguese" },
  { code: "it", label: "Italian" },
  { code: "zh", label: "Chinese" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
];

export function languageLabel(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.label ?? code;
}

/** Display label of a model id; the id itself when the list does not know it. */
export function modelLabel(
  options: ModelOption[] | undefined,
  id: string | null | undefined
): string | null {
  if (!id) {
    return null;
  }

  return options?.find((o) => o.id === id)?.label ?? id;
}

/**
 * Options for a model select: the server list, plus the saved choice when the list does not
 * contain it (shown as unavailable so the saved value stays visible).
 */
export function modelChoices(
  options: ModelOption[],
  currentId: string | null
): ModelOption[] {
  if (!currentId || options.some((o) => o.id === currentId)) {
    return options;
  }

  return [
    {
      available: false,
      id: currentId,
      label: currentId,
      note: "Not in the current model list",
      recommended: false,
      supports_diarization: false,
    },
    ...options,
  ];
}

/** The `auto_final` choices, recommended first. */
export const AUTO_FINAL_CHOICES: { label: string; value: AutoFinal }[] = [
  {
    label: "When the live transcript is incomplete (recommended)",
    value: "when_live_incomplete",
  },
  { label: "Always", value: "always" },
  { label: "Never (run it from the meeting page)", value: "never" },
];

/** Pill for a meeting with only live rows: complete on the final model, or not. */
export function liveTranscriptLabel(
  meeting: Pick<MeetingMetadata, "live_complete" | "live_model">,
  finalModel: string | null | undefined,
  finalOptions: ModelOption[] | undefined
): string {
  if (
    meeting.live_complete === true &&
    !!meeting.live_model &&
    meeting.live_model === finalModel
  ) {
    return `Live · ${modelLabel(finalOptions, meeting.live_model)} · complete`;
  }

  return "Live transcript (incomplete)";
}

/** Model for a retry: the run's own model while it is still offered, else null (the settings model). */
export function retryModel(
  model: string | null,
  finalOptions: ModelOption[] | undefined
): string | null {
  return model && finalOptions?.some((o) => o.id === model) ? model : null;
}

/** Text of one select item: label, plus "recommended" / "unavailable" markers. */
export function modelOptionText(option: ModelOption): string {
  return `${option.label}${option.recommended ? " · recommended" : ""}${option.available ? "" : " · unavailable"}`;
}

/** Vocabulary hints from free text: comma- or line-separated, trimmed, de-duplicated. */
export function parseKeyterms(text: string): string[] {
  const seen = new Set<string>();
  const terms: string[] = [];

  for (const raw of text.split(/[,\n]/)) {
    const term = raw.trim();
    const key = term.toLowerCase();

    if (term && !seen.has(key)) {
      seen.add(key);
      terms.push(term);
    }
  }

  return terms;
}

export function formatKeyterms(terms: string[]): string {
  return terms.join("\n");
}
