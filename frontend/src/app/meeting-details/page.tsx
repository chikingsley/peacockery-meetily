"use client";

import { LoaderIcon } from "lucide-react";
import { useRouter, useSearchParams } from "next/navigation";
import { useState, useEffect, useCallback, useRef, Suspense } from "react";
import { toast } from "sonner";

import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { useConfig } from "@/contexts/ConfigContext";
import { usePaginatedTranscripts } from "@/hooks/usePaginatedTranscripts";
import { parseSummaryContent } from "@/lib/summary-content";
import { voiceService } from "@/services/voiceService";
import type { MeetingSummary, SummaryProcessResponse } from "@/types";
import type { MeetingMetadata } from "@/types/voice";

import PageContent from "./page-content";

/** After a final transcript with `auto_summary`, how long to wait for Rust's summary run to appear. */
const AUTO_SUMMARY_ATTACH_TRIES = 5;
const AUTO_SUMMARY_ATTACH_DELAY_MS = 1500;

function MeetingDetailsContent() {
  const searchParams = useSearchParams();
  const meetingId = searchParams.get("id");
  const justRecorded = searchParams.get("source") === "recording";
  const { setCurrentMeeting, refetchMeetings } = useSidebar();
  const { voiceSettings } = useConfig();
  const router = useRouter();
  const [meeting, setMeeting] = useState<MeetingMetadata | null>(null);

  const [summaryResponse, setSummaryResponse] =
    useState<SummaryProcessResponse | null>(null);

  const [meetingSummary, setMeetingSummary] = useState<MeetingSummary | null>(
    null
  );

  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [showLiveDraft, setShowLiveDraft] = useState(false);

  const {
    metadata,
    transcripts,
    source,
    isLoading: isLoadingTranscripts,
    isLoadingMore,
    hasMore,
    totalCount,
    loadMore,
    refetch,
    error: transcriptError,
  } = usePaginatedTranscripts({
    meetingId: meetingId || "",
    source: showLiveDraft ? "live" : "best",
  });

  useEffect(() => {
    setShowLiveDraft(false);
  }, [meetingId]);

  // Keep the current view until the selected meeting's first transcript page is ready.
  useEffect(() => {
    if (isLoadingTranscripts || !meetingId || metadata?.id !== meetingId) {
      return;
    }

    setMeeting(metadata);
    setCurrentMeeting({ id: metadata.id, title: metadata.title });
  }, [metadata, meetingId, isLoadingTranscripts, setCurrentMeeting]);

  useEffect(() => {
    if (transcriptError) {
      setError(transcriptError);
    }
  }, [transcriptError]);

  const fetchSummary = useCallback(
    async (id: string, isCancelled: () => boolean = () => false) => {
      try {
        const response = await voiceService.getSummary(id);

        if (isCancelled()) {
          return null;
        }

        setSummaryResponse(response);
        setMeetingSummary(
          response.status === "idle" ? null : parseSummaryContent(response.data)
        );

        return response;
      } catch (error_) {
        if (isCancelled()) {
          return null;
        }

        console.error("Error fetching meeting summary:", error_);
        setMeetingSummary(null);

        return null;
      }
    },
    []
  );

  useEffect(() => {
    setMeeting(null);
    setMeetingSummary(null);
    setSummaryResponse(null);
    setError(null);
    setIsLoading(true);

    if (!meetingId) {
      setError("No meeting selected");
      setIsLoading(false);

      return;
    }

    let cancelled = false;
    fetchSummary(meetingId, () => cancelled)
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false);
        }
      })
      .catch((error_) =>
        console.error("Native event synchronization failed", error_)
      );

    return () => {
      cancelled = true;
    };
  }, [meetingId, fetchSummary]);

  const autoSummaryRef = useRef(false);
  autoSummaryRef.current = !!voiceSettings?.auto_summary;
  const visibleMeetingRef = useRef(meetingId);
  visibleMeetingRef.current = meetingId;

  // A finished final transcript replaces the rows shown. With `auto_summary`, Rust starts the
  // summary right after (§6.6); re-read it until that run shows up so its polling attaches.
  const handleFinalTranscriptDone = useCallback(async () => {
    if (!meetingId) {
      return;
    }

    setShowLiveDraft(false);
    await refetch();
    let response = await fetchSummary(meetingId);

    for (
      let i = 0;
      autoSummaryRef.current &&
      response?.status === "idle" &&
      i < AUTO_SUMMARY_ATTACH_TRIES;
      i++
    ) {
      await new Promise((resolve) => {
        setTimeout(resolve, AUTO_SUMMARY_ATTACH_DELAY_MS);
      });

      if (visibleMeetingRef.current !== meetingId) {
        return;
      }

      response = await fetchSummary(meetingId);
    }
  }, [meetingId, refetch, fetchSummary]);

  const handleRenameSpeaker = useCallback(
    async (from: string, to: string) => {
      if (!meetingId) {
        return;
      }

      const renamed = await voiceService.renameSpeaker(meetingId, from, to);

      if (renamed === 0) {
        toast.warning(`No transcript lines are labelled "${from}"`);
      }

      await refetch();
    },
    [meetingId, refetch]
  );

  if (error) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-center">
          <p className="mb-4 text-destructive">{error}</p>
          <button
            type="button"
            onClick={() => router.push("/")}
            className="rounded bg-info px-4 py-2 text-primary-foreground hover:bg-info"
          >
            Go Back
          </button>
        </div>
      </div>
    );
  }

  if (isLoading || !meeting || meeting.id !== meetingId) {
    return (
      <div className="flex h-full items-center justify-center">
        <LoaderIcon className="size-6 animate-spin" />
      </div>
    );
  }

  return (
    <PageContent
      key={meetingId}
      initialSummary={summaryResponse}
      meeting={meeting}
      summaryData={meetingSummary}
      onMeetingUpdated={refetchMeetings}
      onFinalTranscriptDone={handleFinalTranscriptDone}
      onRenameSpeaker={handleRenameSpeaker}
      justRecorded={justRecorded}
      transcripts={transcripts}
      source={source}
      showingLiveDraft={showLiveDraft}
      onToggleSource={() => setShowLiveDraft((v) => !v)}
      hasMore={hasMore}
      isLoadingMore={isLoadingMore}
      totalCount={totalCount}
      onLoadMore={loadMore}
    />
  );
}

export default function MeetingDetails() {
  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center">
          <LoaderIcon className="size-6 animate-spin" />
        </div>
      }
    >
      <MeetingDetailsContent />
    </Suspense>
  );
}
