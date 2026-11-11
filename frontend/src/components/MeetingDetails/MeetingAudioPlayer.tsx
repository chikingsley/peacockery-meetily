'use client';

import { forwardRef, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
/** The meeting's audio file through the asset protocol; shows a note when it cannot be played. */
export const MeetingAudioPlayer = forwardRef<HTMLAudioElement, {
  audioPath: string;
  onTimeUpdate: (seconds: number) => void;
}>(function MeetingAudioPlayer({ audioPath, onTimeUpdate }, ref) {
  const [failed, setFailed] = useState(false);
  if (failed) return <p className="text-xs text-gray-400 text-center">Recording audio not available</p>;
  return (
    <audio
      ref={ref}
      controls
      preload="metadata"
      className="w-full h-9"
      src={convertFileSrc(audioPath)}
      onError={() => setFailed(true)}
      onTimeUpdate={(e) => onTimeUpdate(e.currentTarget.currentTime)}
    />
  );
});
