"use client";

import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useRef,
  useCallback,
  useMemo,
} from "react";
import { toast } from "sonner";

import { voiceService } from "@/services/voiceService";

/**
 * Single source of truth for the recording lifecycle, synced with the Rust core through
 * events (§9) plus a poll of `get_recording_state` while recording (durations).
 */

export enum RecordingStatus {
  IDLE = "idle",
  STARTING = "starting",
  RECORDING = "recording",
  STOPPING = "stopping",
  ERROR = "error",
}

interface RecordingState {
  isRecording: boolean;
  isPaused: boolean;
  isActive: boolean;
  recordingDuration: number | null;
  activeDuration: number | null;
  /** Meeting being recorded (from `recording-started` / `get_recording_state` sync). */
  meetingId: string | null;
  status: RecordingStatus;
  statusMessage?: string;
}

interface RecordingStateContextType extends RecordingState {
  setStatus: (status: RecordingStatus, message?: string) => void;
  setMeetingId: (meetingId: string | null) => void;
  isStopping: boolean;
  isStartingRecording: boolean;
}

const RecordingStateContext = createContext<RecordingStateContextType | null>(
  null
);

export const useRecordingState = () => {
  const context = useContext(RecordingStateContext);

  if (!context) {
    throw new Error(
      "useRecordingState must be used within a RecordingStateProvider"
    );
  }

  return context;
};

const POLL_MS = 500;

/** Shown from the stop request until `recording-stopped` (the live transcript drains first). */
export const STOPPING_MESSAGE = "Stopping… finishing live transcript";

export function RecordingStateProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [state, setState] = useState<RecordingState>({
    activeDuration: null,
    isActive: false,
    isPaused: false,
    isRecording: false,
    meetingId: null,
    recordingDuration: null,
    status: RecordingStatus.IDLE,
    statusMessage: undefined,
  });

  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  // Bumped by every lifecycle event, so a poll answered after one cannot overwrite it.
  const lifecycleEpochRef = useRef(0);
  const isRecordingRef = useRef(false);
  useEffect(() => {
    isRecordingRef.current = state.isRecording;
  }, [state.isRecording]);

  const setStatus = useCallback((status: RecordingStatus, message?: string) => {
    setState((prev) => ({ ...prev, status, statusMessage: message }));
  }, []);

  const setMeetingId = useCallback((meetingId: string | null) => {
    setState((prev) => ({ ...prev, meetingId }));
  }, []);

  /** Returns whether a recording is running or still stopping (so the caller keeps polling). */
  const syncWithBackend = useCallback(async (): Promise<boolean> => {
    const epoch = lifecycleEpochRef.current;

    try {
      const backend = await voiceService.getRecordingState();

      if (epoch !== lifecycleEpochRef.current) {
        return true;
      }

      setState((prev) => {
        let { status, statusMessage } = prev;

        if (backend.is_stopping) {
          status = RecordingStatus.STOPPING;
          statusMessage = STOPPING_MESSAGE;
        } else if (backend.is_recording && status === RecordingStatus.IDLE) {
          status = RecordingStatus.RECORDING;
          statusMessage = undefined;
        }

        return {
          ...prev,
          activeDuration: backend.active_duration,
          isActive: backend.is_active,
          isPaused: backend.is_paused,
          isRecording: backend.is_recording,
          meetingId: backend.meeting_id ?? prev.meetingId,
          recordingDuration: backend.recording_duration,
          status,
          statusMessage,
        };
      });

      return backend.is_recording || backend.is_stopping;
    } catch (error) {
      console.error("[RecordingState] get_recording_state failed:", error);

      return false;
    }
  }, []);

  const startPolling = useCallback(() => {
    if (pollingRef.current) {
      clearInterval(pollingRef.current);
    }

    pollingRef.current = setInterval(() => void syncWithBackend(), POLL_MS);
  }, [syncWithBackend]);

  const stopPolling = useCallback(() => {
    if (pollingRef.current) {
      clearInterval(pollingRef.current);
      pollingRef.current = null;
    }
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
      keep(
        await voiceService.onRecordingStarting(() => {
          setState((prev) =>
            prev.status === RecordingStatus.RECORDING
              ? prev
              : {
                  ...prev,
                  status: RecordingStatus.STARTING,
                  statusMessage: "Starting recording...",
                }
          );
        })
      );
      keep(
        await voiceService.onRecordingStarted(({ meeting_id }) => {
          lifecycleEpochRef.current += 1;
          setState((prev) => ({
            ...prev,
            isActive: true,
            isPaused: false,
            isRecording: true,
            meetingId: meeting_id,
            status: RecordingStatus.RECORDING,
            statusMessage: undefined,
          }));
          startPolling();
        })
      );
      // Stop requested from the button, the tray or app quit: Rust is draining the live transcript.
      keep(
        await voiceService.onRecordingStopping(() => {
          lifecycleEpochRef.current += 1;
          setState((prev) => ({
            ...prev,
            isActive: false,
            isPaused: false,
            isRecording: false,
            status: RecordingStatus.STOPPING,
            statusMessage: STOPPING_MESSAGE,
          }));
        })
      );
      keep(
        await voiceService.onRecordingStopped(() => {
          lifecycleEpochRef.current += 1;
          setState((prev) => ({
            ...prev,
            activeDuration: null,
            isActive: false,
            isPaused: false,
            isRecording: false,
            meetingId: null,
            recordingDuration: null,
            status: RecordingStatus.IDLE,
            statusMessage: undefined,
          }));
          stopPolling();
        })
      );
      keep(
        await voiceService.onRecordingPaused(() => {
          setState((prev) => ({ ...prev, isActive: false, isPaused: true }));
        })
      );
      keep(
        await voiceService.onRecordingResumed(() => {
          setState((prev) => ({ ...prev, isActive: true, isPaused: false }));
        })
      );
      keep(
        await voiceService.onRecordingError(({ message }) => {
          toast.error("Recording error", {
            description: message,
            duration: 10_000,
          });
        })
      );

      // Jabra / Bluetooth mic hot-swap feedback, shown on every page.
      keep(
        await voiceService.onMicDeviceSwitched(({ device_name }) => {
          toast.info(
            `Microphone switched to ${device_name} for this meeting.`,
            { duration: 6000 }
          );
        })
      );
      keep(
        await voiceService.onMicSwapFailed(({ error, device_name }) => {
          if (isRecordingRef.current) {
            toast.error(
              `Microphone fallback failed for ${device_name}: ${error}`,
              {
                duration: 8000,
              }
            );
          }
        })
      );
      keep(
        await voiceService.onMicUnavailable(() => {
          toast.error(
            "No microphone available — recording system audio only.",
            { duration: 8000 }
          );
        })
      );
      keep(
        await voiceService.onMicRecoveryExhausted(({ device_name }) => {
          if (isRecordingRef.current) {
            toast.error(
              `Microphone '${device_name}' could not be recovered — recording continues without a microphone. Stop and restart to fix.`,
              { duration: 10_000 }
            );
          }
        })
      );
    })().catch((error) =>
      console.error("[RecordingState] Failed to set up listeners:", error)
    );

    // Fixes reload desync: the backend may already be recording.
    void syncWithBackend().then((active) => {
      if (!cancelled && active) {
        startPolling();
      }
    });

    return () => {
      cancelled = true;
      unsubscribers.forEach((fn) => fn());
      stopPolling();
    };
  }, [startPolling, stopPolling, syncWithBackend]);

  const value = useMemo<RecordingStateContextType>(
    () => ({
      ...state,
      isStartingRecording: state.status === RecordingStatus.STARTING,
      isStopping: state.status === RecordingStatus.STOPPING,
      setMeetingId,
      setStatus,
    }),
    [state, setStatus, setMeetingId]
  );

  return (
    <RecordingStateContext.Provider value={value}>
      {children}
    </RecordingStateContext.Provider>
  );
}
