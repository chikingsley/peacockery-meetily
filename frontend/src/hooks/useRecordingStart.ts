import { useCallback, useEffect, useRef } from "react";
import { toast } from "sonner";

import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { useConfig } from "@/contexts/ConfigContext";
import {
  useRecordingState,
  RecordingStatus,
} from "@/contexts/RecordingStateContext";
import { useTranscripts } from "@/contexts/TranscriptContext";
import { voiceService, errorMessage } from "@/services/voiceService";

function generateMeetingTitle(now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");

  return `Meeting ${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`;
}

/**
 * Starts a recording from the button, the sidebar event or the auto-start flag.
 * Live transcription needs a Peacockery Voice key; without one `onVoiceNotConfigured` runs.
 */
export function useRecordingStart(onVoiceNotConfigured: () => void) {
  // Synchronous latch: a double click re-enters before any state update lands.
  const isStartingRef = useRef(false);
  const { clearTranscripts, setMeetingTitle } = useTranscripts();

  const { setIsMeetingActive, requestedStart, setRequestedStart } =
    useSidebar();

  const { selectedDevices, refreshKeyStatus } = useConfig();

  const { isRecording, isStopping, setStatus, setMeetingId } =
    useRecordingState();

  const startRecording = useCallback(
    async (meetingTitle?: string) => {
      if (isStartingRef.current || isRecording || isStopping) {
        return;
      }

      isStartingRef.current = true;

      try {
        const keyStatus = await refreshKeyStatus();

        if (!keyStatus?.configured) {
          onVoiceNotConfigured();

          return;
        }

        const title = meetingTitle?.trim() || generateMeetingTitle();
        setStatus(RecordingStatus.STARTING, "Starting recording...");
        clearTranscripts();
        setMeetingTitle(title);

        const result = await voiceService.startRecording(
          selectedDevices.micDevice,
          selectedDevices.systemDevice,
          title
        );

        setMeetingId(result.meeting_id);
        setIsMeetingActive(true);
      } catch (error) {
        const message = errorMessage(error);

        if (message.includes("already in progress")) {
          // Another start won the race and is live.
          setStatus(RecordingStatus.RECORDING);

          return;
        }

        setStatus(RecordingStatus.ERROR, message);
        toast.error("Failed to start recording", {
          description: message,
          duration: 10_000,
        });
      } finally {
        isStartingRef.current = false;
      }
    },
    [
      isRecording,
      isStopping,
      refreshKeyStatus,
      onVoiceNotConfigured,
      setStatus,
      clearTranscripts,
      setMeetingTitle,
      selectedDevices,
      setMeetingId,
      setIsMeetingActive,
    ]
  );

  useEffect(() => {
    if (requestedStart) {
      setRequestedStart(false);
      void startRecording();
    }
  }, [requestedStart, setRequestedStart, startRecording]);

  return { startRecording };
}
