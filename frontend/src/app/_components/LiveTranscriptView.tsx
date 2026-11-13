import { motion } from "framer-motion";
import { memo } from "react";

import { SpeakerChip } from "@/components/SpeakerChip";
import { formatRecordingTime } from "@/lib/transcript-format";
import type { LiveLine, TranscriptInterim } from "@/types/voice";

interface LiveTranscriptViewProps {
  lines: LiveLine[];
  interims: TranscriptInterim[];
  isRecording: boolean;
  isPaused: boolean;
  isStopping: boolean;
  liveEnabled: boolean;
}

const Line = memo(({ line }: { line: LiveLine }) => (
  <motion.div
    initial={{ opacity: 0, y: 4 }}
    animate={{ opacity: 1, y: 0 }}
    transition={{ duration: 0.15 }}
    className="mb-3 flex items-start gap-2"
  >
    <span className="mt-1 min-w-[50px] flex-shrink-0 text-xs text-muted-foreground tabular-nums">
      {formatRecordingTime(line.audio_start_time)}
    </span>
    <div className="flex-1">
      {line.speaker && (
        <span className="mr-2">
          <SpeakerChip speaker={line.speaker} />
        </span>
      )}
      <span className="text-base leading-relaxed text-foreground">
        {line.text}
      </span>
    </div>
  </motion.div>
));

/** Finalized live lines plus one italic preview line per channel. The parent scrolls. */
export function LiveTranscriptView({
  lines,
  interims,
  isRecording,
  isPaused,
  isStopping,
  liveEnabled,
}: LiveTranscriptViewProps) {
  const empty = lines.length === 0 && interims.length === 0;

  return (
    <div className="px-4 py-2">
      {empty ? (
        <div className="mt-8 text-center text-muted-foreground">
          {isRecording ? (
            <>
              <div className="mb-3 flex items-center justify-center">
                <div
                  className={`h-3 w-3 rounded-full ${isPaused ? "bg-warning" : "animate-pulse bg-info"}`}
                />
              </div>
              <p className="text-sm text-muted-foreground">
                {isPaused
                  ? "Recording paused"
                  : liveEnabled
                    ? "Listening for speech…"
                    : "Recording (live transcript is off)"}
              </p>
              <p className="mt-1 text-xs text-muted-foreground">
                {isPaused
                  ? "Resume to continue recording"
                  : liveEnabled
                    ? "Live lines appear here as people speak"
                    : "The final transcript runs after you stop"}
              </p>
            </>
          ) : (
            <>
              <p className="text-lg font-semibold">Peacockery Meetily</p>
              <p className="mt-1 text-xs">
                Start recording to see the live transcript
              </p>
            </>
          )}
        </div>
      ) : (
        <div className="pt-2">
          {lines.map((line) => (
            <Line key={`${line.meeting_id}-${line.sequence_id}`} line={line} />
          ))}
          {interims.map((interim) => (
            <div
              key={interim.channel}
              className="mb-3 flex items-start gap-2 text-muted-foreground italic"
            >
              <span className="mt-1 min-w-[50px] flex-shrink-0 text-xs text-muted-foreground">
                [live]
              </span>
              <div className="flex-1">
                {interim.speaker && (
                  <span className="mr-2">
                    <SpeakerChip speaker={interim.speaker} />
                  </span>
                )}
                <span className="text-base leading-relaxed">
                  {interim.text}
                </span>
              </div>
            </div>
          ))}
          {!isStopping && isRecording && !isPaused && interims.length === 0 && (
            <div className="mt-4 flex items-center gap-2 text-muted-foreground">
              <div className="h-2 w-2 animate-pulse rounded-full bg-info" />
              <span className="text-sm">Listening…</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

Line.displayName = "LiveTranscriptLine";
