"use client";

import { useRouter } from "next/navigation";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
} from "react";
import type { ReactNode } from "react";
import { toast } from "sonner";

import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { voiceService, errorMessage } from "@/services/voiceService";

import {
  useRecordingState,
  RecordingStatus,
  STOPPING_MESSAGE,
} from "./RecordingStateContext";

/**
 * The one owner of "recording stopped": the Stop button calls `stopRecording()`, and stops from
 * the tray or a shortcut arrive as `recording-stopped`. Either way the stopped meeting opens
 * once in /meeting-details, where its final transcript status shows (Rust starts the final
 * transcript itself when `auto_final` is on).
 */

interface RecordingStopContextType {
  stopRecording: () => Promise<void>;
}

const RecordingStopContext = createContext<RecordingStopContextType | null>(
  null
);

export function useRecordingStop() {
  const ctx = useContext(RecordingStopContext);

  if (!ctx) {
    throw new Error(
      "useRecordingStop must be used within RecordingStopProvider"
    );
  }

  return ctx;
}

export function RecordingStopProvider({ children }: { children: ReactNode }) {
  const router = useRouter();
  const { setStatus } = useRecordingState();

  const { refetchMeetings, setCurrentMeeting, setIsMeetingActive } =
    useSidebar();

  const openedMeetingIdRef = useRef<string | null>(null);
  const stopInProgressRef = useRef(false);

  const openStoppedMeeting = useCallback(
    async (meetingId: string, title?: string) => {
      if (openedMeetingIdRef.current === meetingId) {
        return;
      }

      openedMeetingIdRef.current = meetingId;
      setIsMeetingActive(false);
      await refetchMeetings();

      if (title) {
        setCurrentMeeting({ id: meetingId, title });
      }

      router.push(
        `/meeting-details?id=${encodeURIComponent(meetingId)}&source=recording`
      );
    },
    [refetchMeetings, router, setCurrentMeeting, setIsMeetingActive]
  );

  const stopRecording = useCallback(async () => {
    if (stopInProgressRef.current) {
      return;
    }

    stopInProgressRef.current = true;
    setStatus(RecordingStatus.STOPPING, STOPPING_MESSAGE);

    try {
      // Resolves once the live transcript is drained and the audio is saved. A failed save still
      // resolves (audio_path null) and is reported once through `recording-error`.
      const result = await voiceService.stopRecording();
      setStatus(RecordingStatus.IDLE);
      await openStoppedMeeting(result.meeting_id);
    } catch (error) {
      // Only "No recording in progress": nothing is stopping, so the controls go back to idle.
      setStatus(RecordingStatus.IDLE);
      toast.error("Failed to stop recording", {
        description: errorMessage(error),
      });
    } finally {
      stopInProgressRef.current = false;
    }
  }, [openStoppedMeeting, setStatus]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    voiceService
      .onRecordingStopped(({ meeting_id, meeting_name }) => {
        void openStoppedMeeting(meeting_id, meeting_name);
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((error) =>
        console.error("Native event synchronization failed", error)
      );

    const resetOnStart = voiceService.onRecordingStarted(() => {
      openedMeetingIdRef.current = null;
    });

    return () => {
      cancelled = true;
      unlisten?.();
      resetOnStart
        .then((fn) => fn())
        .catch((error) =>
          console.error("Native event synchronization failed", error)
        );
    };
  }, [openStoppedMeeting]);

  return (
    <RecordingStopContext.Provider value={{ stopRecording }}>
      {children}
    </RecordingStopContext.Provider>
  );
}
