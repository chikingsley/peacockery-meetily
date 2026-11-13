import { afterEach, describe, expect, mock, test } from "bun:test";

import { blocksToMarkdownSafely } from "../../src/lib/blocknote-markdown";

describe("blocksToMarkdownSafely", () => {
  afterEach(() => {
    mock.restore();
  });

  test("returns markdown when conversion succeeds", async () => {
    const editor = {
      blocksToMarkdownLossy: mock(async () => "# Summary"),
    };

    const result = await blocksToMarkdownSafely(editor, [] as any, {
      source: "test-success",
    });

    expect(result).toEqual({
      markdown: "# Summary",
      ok: true,
    });
    expect(editor.blocksToMarkdownLossy).toHaveBeenCalledTimes(1);
  });

  test("returns fallback markdown when conversion throws", async () => {
    const error = new Error("conversion failed");

    const editor = {
      blocksToMarkdownLossy: mock(async () => {
        throw error;
      }),
    };

    const consoleError = mock(() => {});
    console.error = consoleError as any;

    const result = await blocksToMarkdownSafely(
      editor,
      [{ id: "block-1" }] as any,
      {
        fallbackMarkdown: "existing markdown",
        source: "test-fallback",
      }
    );

    expect(result).toEqual({
      markdown: "existing markdown",
      ok: false,
    });
    expect(consoleError).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalledWith(
      "Failed to convert BlockNote blocks to markdown",
      {
        blocksCount: 1,
        error,
        source: "test-fallback",
      }
    );
  });

  test("omits markdown when conversion throws without fallback", async () => {
    const editor = {
      blocksToMarkdownLossy: mock(async () => {
        throw new Error("conversion failed");
      }),
    };

    console.error = mock(() => {}) as any;

    const result = await blocksToMarkdownSafely(editor, [] as any, {
      source: "test-empty-fallback",
    });

    expect(result).toEqual({
      markdown: undefined,
      ok: false,
    });
  });
});
