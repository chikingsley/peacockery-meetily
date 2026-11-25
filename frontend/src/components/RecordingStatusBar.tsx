"use client";

import { motion } from "framer-motion";
import { useEffect, useState } from "react";

import { useRecordingState } from "@/contexts/RecordingStateContext";

interface RecordingStatusBarProps {
  isPaused?: boolean;
}

export const RecordingStatusBar: React.FC<RecordingStatusBarProps> = ({
  isPaused = false,
}) => {
  // Get recording duration from backend-synced context (in seconds)
  // Backend polls every 500ms, providing smooth updates
  const { activeDuration, progress } = useRecordingState();

  // Display state synced from backend
  const [displaySeconds, setDisplaySeconds] = useState(0);

  // Sync with backend duration when it changes (handles refresh/navigation)
  useEffect(() => {
    if (activeDuration !== null) {
      // Round to nearest second to avoid decimal issues
      setDisplaySeconds(Math.floor(activeDuration));
    }
  }, [activeDuration]);

  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;

    return `${mins.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
  };

  const behind = progress && (activeDuration ?? 0) - progress.received > 5;

  let health = progress?.received
    ? "Recording locally"
    : "Waiting for local audio";

  if (behind) {
    health = "Local audio behind";
  }

  const micGap = progress?.mic_gap_seconds ?? 0;

  if (micGap > 0) {
    const gap = `Microphone capture gap · ${micGap.toFixed(1)}s total`;
    health = behind ? `${health} · ${gap}` : gap;
  }

  if (progress?.write_failed) {
    health = "Audio save failed";
  }

  if (isPaused) {
    health = "Paused";
  }

  const warning = isPaused || micGap > 0 || behind || progress?.write_failed;

  return (
    <motion.div
      initial={{ opacity: 0, y: -10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.2 }}
      className="flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-1.5"
    >
      <div
        className={`h-2 w-2 rounded-full ${warning ? "bg-warning" : "animate-pulse bg-destructive"}`}
      />
      <span
        className={`text-sm ${warning ? "text-warning" : "text-foreground"}`}
      >
        {health} • {formatDuration(displaySeconds)}
        {progress && (
          <> · On disk {formatDuration(Math.floor(progress.saved))}</>
        )}
      </span>
    </motion.div>
  );
};
