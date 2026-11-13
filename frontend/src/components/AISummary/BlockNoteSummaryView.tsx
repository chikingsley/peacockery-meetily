"use client";

import { useCreateBlockNote } from "@blocknote/react";
import { BlockNoteView } from "@blocknote/shadcn";
import { useTheme } from "next-themes";
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { toast } from "sonner";

import { blocksToMarkdownSafely } from "@/lib/blocknote-markdown";
import type { SummaryDataResponse, BlockNoteBlock } from "@/types";

import "@blocknote/shadcn/style.css";

interface BlockNoteSummaryViewProps {
  summaryData: SummaryDataResponse | null;
  onSave?: (data: {
    markdown?: string;
    summary_json?: BlockNoteBlock[];
  }) => Promise<void>;
  onDirtyChange?: (isDirty: boolean) => void;
}

export interface BlockNoteSummaryViewRef {
  saveSummary: () => Promise<void>;
  getMarkdown: () => Promise<string>;
  isDirty: boolean;
}

export const BlockNoteSummaryView = forwardRef<
  BlockNoteSummaryViewRef,
  BlockNoteSummaryViewProps
>(({ summaryData, onSave, onDirtyChange }, ref) => {
  const editor = useCreateBlockNote();
  const { resolvedTheme } = useTheme();
  const [isDirty, setIsDirty] = useState(false);
  const loading = useRef(false);
  const savedDocument = useRef("");

  useEffect(() => {
    loading.current = true;

    try {
      const blocks = summaryData?.summary_json?.length
        ? summaryData.summary_json
        : editor.tryParseMarkdownToBlocks(summaryData?.markdown ?? "");

      editor.replaceBlocks(
        editor.document,
        blocks.length ? blocks : [{ type: "paragraph" }]
      );
      savedDocument.current = JSON.stringify(editor.document);
      setIsDirty(false);
    } catch (error) {
      toast.error("Summary could not be opened", {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      loading.current = false;
    }
  }, [editor, summaryData]);

  useEffect(() => {
    onDirtyChange?.(isDirty);
  }, [isDirty, onDirtyChange]);

  const getMarkdown = useCallback(async () => {
    const result = await blocksToMarkdownSafely(editor, editor.document, {
      fallbackMarkdown: summaryData?.markdown,
      source: "summary-editor",
    });

    return result.markdown ?? "";
  }, [editor, summaryData?.markdown]);

  const saveSummary = useCallback(async () => {
    if (!onSave || !isDirty) {
      return;
    }

    const { document } = editor;
    const fingerprint = JSON.stringify(document);

    const result = await blocksToMarkdownSafely(editor, document, {
      source: "summary-save",
    });

    await onSave({ summary_json: document, markdown: result.markdown });
    savedDocument.current = fingerprint;
    setIsDirty(JSON.stringify(editor.document) !== fingerprint);
  }, [editor, isDirty, onSave]);

  useImperativeHandle(ref, () => ({ getMarkdown, isDirty, saveSummary }), [
    getMarkdown,
    isDirty,
    saveSummary,
  ]);

  return (
    <BlockNoteView
      editor={editor}
      theme={resolvedTheme === "dark" ? "dark" : "light"}
      onChange={() => {
        if (!loading.current) {
          setIsDirty(JSON.stringify(editor.document) !== savedDocument.current);
        }
      }}
    />
  );
});

BlockNoteSummaryView.displayName = "BlockNoteSummaryView";
