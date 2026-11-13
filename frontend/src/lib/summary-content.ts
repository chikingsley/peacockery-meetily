import { z } from "zod";

import type { MeetingSummary } from "@/types";

const REASONING_MARKER = /<\/?think(?:ing)?(?:\s+[^>]*)?>/i;
const SummaryObject = z.record(z.string(), z.unknown());

function parseSummaryObject(value: unknown) {
  const parsed = SummaryObject.safeParse(value);

  return parsed.success ? parsed.data : null;
}

function containsReasoningMarker(value: unknown): boolean {
  if (Array.isArray(value)) {
    return value.some(containsReasoningMarker);
  }

  const object = parseSummaryObject(value);

  if (!object) {
    return false;
  }

  return Object.entries(object).some(([key, child]) => {
    if (typeof child === "string") {
      return (
        ["markdown", "text", "content"].includes(key) &&
        REASONING_MARKER.test(child)
      );
    }

    return (
      ["summary_json", "children", "content"].includes(key) &&
      containsReasoningMarker(child)
    );
  });
}

function hasBlockText(value: unknown): boolean {
  if (typeof value === "string") {
    return value.trim().length > 0;
  }

  if (Array.isArray(value)) {
    return value.some(hasBlockText);
  }

  const object = parseSummaryObject(value);

  return object
    ? [object.text, object.content, object.children].some(hasBlockText)
    : false;
}

export function hasVisibleSummaryContent(
  value: unknown
): value is MeetingSummary {
  const object = parseSummaryObject(value);

  if (!object || containsReasoningMarker(value)) {
    return false;
  }

  if ("markdown" in object) {
    return (
      typeof object.markdown === "string" && object.markdown.trim().length > 0
    );
  }

  if ("summary_json" in object) {
    return hasBlockText(object.summary_json);
  }

  return false;
}

const SummaryMetadata = z
  .object({
    reasoning_stripped: z.boolean().optional(),
  })
  .passthrough();

export function readSummaryMetadata(value: unknown) {
  const parsed = SummaryMetadata.safeParse(value);

  return parsed.success
    ? { reasoningStripped: parsed.data.reasoning_stripped === true }
    : { reasoningStripped: false };
}

export function parseSummaryContent(input: unknown): MeetingSummary | null {
  let value = input;

  if (typeof value === "string") {
    try {
      value = JSON.parse(value);
    } catch {
      return null;
    }
  }

  return hasVisibleSummaryContent(value) ? value : null;
}
