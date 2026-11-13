"use client";

import { Loader2, Mic, Pause, Play, Square } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
  useRecordingState,
  STOPPING_MESSAGE,
} from "@/contexts/RecordingStateContext";
import { useRecordingStop } from "@/contexts/RecordingStopContext";
import { useAudioLevels } from "@/hooks/useAudioLevels";
import { SYSTEM_SILENCE_WARN_SECONDS } from "@/lib/live-transcript";
import { voiceService, errorMessage } from "@/services/voiceService";

export function RecordingControls({
  onStart,
}: {
  onStart: () => Promise<void>;
}) {
  const { isRecording, isPaused, isStartingRecording, isStopping } =
    useRecordingState();

  const { stopRecording } = useRecordingStop();
  const [toggling, setToggling] = useState(false);
  const levels = useAudioLevels(isRecording);

  async function togglePause() {
    setToggling(true);

    try {
      if (isPaused) {
        await voiceService.resumeRecording();
      } else {
        await voiceService.pauseRecording();
      }
    } catch (error) {
      toast.error("Recording control failed", {
        description: errorMessage(error),
      });
    } finally {
      setToggling(false);
    }
  }

  if (isStopping) {
    return (
      <div role="status" className="flex items-center gap-3 text-sm">
        <Loader2 className="size-4 animate-spin" />
        {STOPPING_MESSAGE}
      </div>
    );
  }

  const busy = toggling || isStartingRecording;

  return (
    <div className="flex flex-wrap items-center justify-center gap-4">
      {isRecording ? (
        <>
          <Button variant="outline" disabled={busy} onClick={togglePause}>
            {isPaused ? <Play /> : <Pause />}
            {isPaused ? "Resume" : "Pause"}
          </Button>
          <Button variant="destructive" disabled={busy} onClick={stopRecording}>
            <Square />
            Stop recording
          </Button>
        </>
      ) : (
        <Button onClick={onStart} disabled={busy}>
          {isStartingRecording ? <Loader2 className="animate-spin" /> : <Mic />}
          {isStartingRecording ? "Starting…" : "Start recording"}
        </Button>
      )}
      <LevelBar label="Microphone" level={isPaused ? 0 : levels.mic} />
      <LevelBar
        label="System audio"
        level={isPaused ? 0 : levels.system}
        warn={
          isRecording &&
          !isPaused &&
          levels.system_silent_seconds > SYSTEM_SILENCE_WARN_SECONDS
        }
      />
    </div>
  );
}

function LevelBar({
  label,
  level,
  warn = false,
}: {
  label: string;
  level: number;
  warn?: boolean;
}) {
  return (
    <div className="space-y-1">
      <span
        className={
          warn ? "text-xs text-warning" : "text-xs text-muted-foreground"
        }
      >
        {label}
        {warn ? " · Check source" : ""}
      </span>
      <meter
        className="block h-2 w-24"
        min={0}
        max={1}
        value={Math.min(1, Math.max(0, level))}
        aria-label={`${label} level`}
      />
    </div>
  );
}
