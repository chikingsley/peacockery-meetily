"use client";

import type { ReactNode } from "react";
import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useRef,
} from "react";
import { toast } from "sonner";

import {
  applyInterim,
  applyLiveStatus,
  insertLiveLine,
} from "@/lib/live-transcript";
import type { InterimsByChannel, StatusByChannel } from "@/lib/live-transcript";
import { formatTranscriptText } from "@/lib/transcript-format";
import { voiceService } from "@/services/voiceService";
import type { LiveLine } from "@/types/voice";

import { useRecordingState } from "./RecordingStateContext";

/**
 * Live transcript of the current recording. Rust persists every finalized line itself
 * (§5, §7); this context only renders `transcript-update` / `transcript-interim` and
 * resyncs from `get_transcript_history` after a reload.
 */

interface TranscriptContextType {
  /** Finalized live lines, ordered by audio time. */
  lines: LiveLine[];
  /** Current preview text per channel (cleared with an empty interim). */
  interims: InterimsByChannel;
  /** Latest live-session state per channel. */
  liveStatus: StatusByChannel;
  transcriptContainerRef: React.RefObject<HTMLDivElement | null>;
  meetingTitle: string;
  setMeetingTitle: (title: string) => void;
  copyTranscript: () => void;
  clearTranscripts: () => void;
}

const TranscriptContext = createContext<TranscriptContextType | undefined>(
  undefined
);

export function TranscriptProvider({ children }: { children: ReactNode }) {
  const [lines, setLines] = useState<LiveLine[]>([]);
  const [interims, setInterims] = useState<InterimsByChannel>({});
  const [liveStatus, setLiveStatus] = useState<StatusByChannel>({});
  const [meetingTitle, setMeetingTitle] = useState("+ New Call");
  const transcriptContainerRef = useRef<HTMLDivElement>(null);
  const { isRecording } = useRecordingState();

  const clearTranscripts = useCallback(() => {
    setLines([]);
    setInterims({});
    setLiveStatus({});
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unsubscribers: (() => void)[] = [];

    const keep = (fn: () => void) => {
      if (cancelled) {
        fn();
      } else {
        unsubscribers.push(fn);
      }
    };

    (async () => {
      // Cleared at `recording-starting`: the first live status events arrive before
      // `recording-started` and must survive it.
      keep(await voiceService.onRecordingStarting(clearTranscripts));
      keep(
        await voiceService.onRecordingStarted(async () => {
          const name = await voiceService
            .getRecordingMeetingName()
            .catch(() => null);

          if (name) {
            setMeetingTitle(name);
          }
        })
      );
      keep(
        await voiceService.onTranscriptUpdate((line) => {
          setLines((prev) => insertLiveLine(prev, line));
        })
      );
      keep(
        await voiceService.onTranscriptInterim((interim) => {
          setInterims((prev) => applyInterim(prev, interim));
        })
      );
      keep(
        await voiceService.onLiveTranscriptionStatus((status) => {
          setLiveStatus((prev) => applyLiveStatus(prev, status));

          if (status.state === "failed") {
            toast.error(`Live transcription (${status.channel}) failed`, {
              description:
                status.message ??
                "Audio is still being recorded; the final transcript runs after stop.",
              duration: 8000,
            });
          }
        })
      );
    })().catch((error) =>
      console.error("[TranscriptContext] Failed to set up listeners:", error)
    );

    return () => {
      cancelled = true;
      unsubscribers.forEach((fn) => fn());
    };
  }, [clearTranscripts]);

  // After a reload during recording, rebuild the view from the Rust session history.
  useEffect(() => {
    if (!isRecording) {
      return;
    }

    let cancelled = false;
    (async () => {
      try {
        const [history, name] = await Promise.all([
          voiceService.getTranscriptHistory(),
          voiceService.getRecordingMeetingName(),
        ]);

        if (cancelled) {
          return;
        }

        setLines((prev) => history.reduce(insertLiveLine, prev));

        if (name) {
          setMeetingTitle(name);
        }
      } catch (error) {
        console.error(
          "[TranscriptContext] Failed to sync transcript history:",
          error
        );
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [isRecording]);

  // Keep the view pinned to the newest line while the user is at the bottom.
  useEffect(() => {
    const container = transcriptContainerRef.current;

    if (!container) {
      return;
    }

    const atBottom =
      container.scrollTop + container.clientHeight >=
      container.scrollHeight - 80;

    if (atBottom) {
      container.scrollTo({ behavior: "smooth", top: container.scrollHeight });
    }
  }, [lines, interims]);

  const copyTranscript = useCallback(() => {
    const text = formatTranscriptText(lines);
    navigator.clipboard.writeText(text);
    toast.success("Transcript copied to clipboard");
  }, [lines]);

  const value: TranscriptContextType = {
    clearTranscripts,
    copyTranscript,
    interims,
    lines,
    liveStatus,
    meetingTitle,
    setMeetingTitle,
    transcriptContainerRef,
  };

  return (
    <TranscriptContext.Provider value={value}>
      {children}
    </TranscriptContext.Provider>
  );
}

export function useTranscripts() {
  const context = useContext(TranscriptContext);

  if (context === undefined) {
    throw new Error("useTranscripts must be used within a TranscriptProvider");
  }

  return context;
}
