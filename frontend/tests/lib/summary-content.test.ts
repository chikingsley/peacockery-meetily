import { describe, expect, test } from "bun:test";

import {
  hasVisibleSummaryContent,
  parseSummaryContent,
} from "../../src/lib/summary-content";

describe("summary content validation", () => {
  test("accepts visible markdown and rejects whitespace or reasoning markers", () => {
    expect(hasVisibleSummaryContent({ markdown: "# Summary\nVisible" })).toBe(
      true
    );
    expect(hasVisibleSummaryContent({ markdown: "   " })).toBe(false);
    expect(
      hasVisibleSummaryContent({ markdown: "<think>private</think>" })
    ).toBe(false);
    expect(
      hasVisibleSummaryContent({
        markdown: '<thinking class="internal">private</thinking>',
      })
    ).toBe(false);
    expect(
      hasVisibleSummaryContent({ markdown: "<thinker>Visible</thinker>" })
    ).toBe(true);
  });

  test("validates BlockNote content and rejects other shapes", () => {
    expect(hasVisibleSummaryContent({ summary_json: [] })).toBe(false);
    expect(
      hasVisibleSummaryContent({
        summary_json: [{ content: [{ text: "Visible", type: "text" }] }],
      })
    ).toBe(true);
    expect(
      hasVisibleSummaryContent({
        Decisions: { blocks: [{ content: "Ship it" }] },
      })
    ).toBe(false);
  });

  test("parses a JSON-encoded string payload", () => {
    expect(parseSummaryContent('{"markdown":"Visible"}')).toEqual({
      markdown: "Visible",
    });
    expect(parseSummaryContent("{not json")).toBeNull();
  });
});
