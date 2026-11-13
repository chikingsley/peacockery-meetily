import type { RefObject } from "react";
import { useCallback } from "react";
import { toast } from "sonner";

import type { BlockNoteSummaryViewRef } from "@/components/AISummary/BlockNoteSummaryView";
import { hasVisibleSummaryContent } from "@/lib/summary-content";
import { formatTranscriptText } from "@/lib/transcript-format";
import { voiceService } from "@/services/voiceService";
import type { MeetingSummary } from "@/types";
import type {
  MeetingMetadata,
  Transcript,
  TranscriptSourceQuery,
} from "@/types/voice";

interface UseCopyOperationsProps {
  meeting: MeetingMetadata;
  meetingTitle: string;
  aiSummary: MeetingSummary | null;
  blockNoteSummaryRef: RefObject<BlockNoteSummaryViewRef | null>;
  /** The transcript on screen: best (final when present) or the live draft. */
  transcriptSource: TranscriptSourceQuery;
}

const dateTime = (value: string | null) =>
  value
    ? new Date(value).toLocaleDateString("en-US", {
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        month: "long",
        year: "numeric",
      })
    : "";

/** Every row of one transcript source of the meeting. */
async function fetchAllTranscripts(
  meetingId: string,
  source: TranscriptSourceQuery
): Promise<Transcript[]> {
  const first = await voiceService.getMeetingTranscripts(
    meetingId,
    1,
    0,
    source
  );

  if (first.total_count === 0) {
    return [];
  }

  const all = await voiceService.getMeetingTranscripts(
    meetingId,
    first.total_count,
    0,
    source
  );

  return all.transcripts;
}

export function useCopyOperations({
  meeting,
  meetingTitle,
  aiSummary,
  blockNoteSummaryRef,
  transcriptSource,
}: UseCopyOperationsProps) {
  const handleCopyTranscript = useCallback(async () => {
    let transcripts: Transcript[];

    try {
      transcripts = await fetchAllTranscripts(meeting.id, transcriptSource);
    } catch (error) {
      console.error("Failed to fetch transcripts for copying:", error);
      toast.error("Failed to fetch transcripts for copying");

      return;
    }

    if (!transcripts.length) {
      toast.error("No transcripts available to copy");

      return;
    }

    const header = `# Transcript: ${meetingTitle || meeting.title}\n\n## Date: ${dateTime(meeting.created_at)}\n\n`;
    const body = formatTranscriptText(transcripts);
    await navigator.clipboard.writeText(header + body);
    toast.success("Transcript copied to clipboard");
  }, [meeting, meetingTitle, transcriptSource]);

  const handleCopySummary = useCallback(async () => {
    if (!hasVisibleSummaryContent(aiSummary)) {
      toast.error("No summary content available to copy");

      return;
    }

    try {
      let markdown = blockNoteSummaryRef.current
        ? await blockNoteSummaryRef.current.getMarkdown()
        : "";

      const { markdown: savedMarkdown } = aiSummary;

      if (!markdown && savedMarkdown) {
        markdown = savedMarkdown;
      }

      if (!markdown.trim()) {
        toast.error("No summary content available to copy");

        return;
      }

      const header = `# Meeting Summary: ${meetingTitle}\n\n**Date:** ${dateTime(meeting.created_at)}\n**Copied on:** ${dateTime(new Date().toISOString())}\n\n---\n\n`;
      await navigator.clipboard.writeText(header + markdown);
      toast.success("Summary copied to clipboard");
    } catch (error) {
      console.error("Failed to copy summary:", error);
      toast.error("Failed to copy summary");
    }
  }, [aiSummary, meetingTitle, meeting, blockNoteSummaryRef]);

  return { handleCopySummary, handleCopyTranscript };
}
