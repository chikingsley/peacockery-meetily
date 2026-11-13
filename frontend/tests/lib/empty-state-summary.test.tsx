import { describe, expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { EmptyStateSummary } from "../../src/components/EmptyStateSummary";

describe("empty summary state", () => {
  test("renders a persistent error beside the summary controls", () => {
    const html = renderToStaticMarkup(
      <EmptyStateSummary error="Summary request failed" />
    );

    expect(html).toContain('role="alert"');
    expect(html).toContain("Summary request failed");
  });
});
