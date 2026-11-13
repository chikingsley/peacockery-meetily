import type { Block } from "@blocknote/core";

interface MarkdownCapableEditor {
  blocksToMarkdownLossy: (blocks: Block[]) => Promise<string> | string;
}

interface MarkdownConversionOptions {
  source: string;
  fallbackMarkdown?: string;
}

interface MarkdownConversionResult {
  markdown?: string;
  ok: boolean;
}

export async function blocksToMarkdownSafely(
  editor: MarkdownCapableEditor,
  blocks: Block[],
  options: MarkdownConversionOptions
): Promise<MarkdownConversionResult> {
  try {
    return {
      markdown: await editor.blocksToMarkdownLossy(blocks),
      ok: true,
    };
  } catch (error) {
    console.error("Failed to convert BlockNote blocks to markdown", {
      blocksCount: blocks.length,
      error,
      source: options.source,
    });

    return {
      markdown: options.fallbackMarkdown,
      ok: false,
    };
  }
}
