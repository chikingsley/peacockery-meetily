"use client";

import { convertFileSrc } from "@tauri-apps/api/core";
import { forwardRef, useState } from "react";

/** The meeting's audio file through the asset protocol; shows a note when it cannot be played. */
export const MeetingAudioPlayer = forwardRef<
  HTMLAudioElement,
  {
    audioPath: string;
    onTimeUpdate: (seconds: number) => void;
  }
>(({ audioPath, onTimeUpdate }, ref) => {
  const [failed, setFailed] = useState(false);

  if (failed) {
    return (
      <p className="text-center text-xs text-muted-foreground">
        Recording audio not available
      </p>
    );
  }

  return (
    // oxlint-disable-next-line jsx-a11y/media-has-caption -- The synchronized transcript is rendered beside this audio player.
    <audio
      ref={ref}
      controls
      preload="metadata"
      className="h-9 w-full"
      src={convertFileSrc(audioPath)}
      onError={() => setFailed(true)}
      onTimeUpdate={(e) => onTimeUpdate(e.currentTarget.currentTime)}
    />
  );
});

MeetingAudioPlayer.displayName = "MeetingAudioPlayer";
