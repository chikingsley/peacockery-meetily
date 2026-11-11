import { memo } from 'react';
import { motion } from 'framer-motion';
import type { LiveLine, TranscriptInterim } from '@/types/voice';
import { SpeakerChip } from '@/components/SpeakerChip';
import { formatRecordingTime } from '@/lib/transcript-format';

interface LiveTranscriptViewProps {
  lines: LiveLine[];
  interims: TranscriptInterim[];
  isRecording: boolean;
  isPaused: boolean;
  isStopping: boolean;
  liveEnabled: boolean;
}

const Line = memo(function Line({ line }: { line: LiveLine }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.15 }}
      className="flex items-start gap-2 mb-3"
    >
      <span className="text-xs text-gray-400 mt-1 flex-shrink-0 min-w-[50px] tabular-nums">
        {formatRecordingTime(line.audio_start_time)}
      </span>
      <div className="flex-1">
        {line.speaker && <span className="mr-2"><SpeakerChip speaker={line.speaker} /></span>}
        <span className="text-base text-gray-800 leading-relaxed">{line.text}</span>
      </div>
    </motion.div>
  );
});

/** Finalized live lines plus one italic preview line per channel. The parent scrolls. */
export function LiveTranscriptView({ lines, interims, isRecording, isPaused, isStopping, liveEnabled }: LiveTranscriptViewProps) {
  const empty = lines.length === 0 && interims.length === 0;
  return (
    <div className="px-4 py-2">
      {empty ? (
        <div className="text-center text-gray-500 mt-8">
          {isRecording ? (
            <>
              <div className="flex items-center justify-center mb-3">
                <div className={`w-3 h-3 rounded-full ${isPaused ? 'bg-orange-500' : 'bg-blue-500 animate-pulse'}`} />
              </div>
              <p className="text-sm text-gray-600">
                {isPaused ? 'Recording paused' : liveEnabled ? 'Listening for speech…' : 'Recording (live transcript is off)'}
              </p>
              <p className="text-xs mt-1 text-gray-400">
                {isPaused
                  ? 'Resume to continue recording'
                  : liveEnabled ? 'Live lines appear here as people speak' : 'The final transcript runs after you stop'}
              </p>
            </>
          ) : (
            <>
              <p className="text-lg font-semibold">Peacockery Meetily</p>
              <p className="text-xs mt-1">Start recording to see the live transcript</p>
            </>
          )}
        </div>
      ) : (
        <div className="pt-2">
          {lines.map(line => <Line key={`${line.meeting_id}-${line.sequence_id}`} line={line} />)}
          {interims.map(interim => (
            <div key={interim.channel} className="flex items-start gap-2 mb-3 text-gray-500 italic">
              <span className="text-xs text-gray-300 mt-1 flex-shrink-0 min-w-[50px]">[live]</span>
              <div className="flex-1">
                {interim.speaker && <span className="mr-2"><SpeakerChip speaker={interim.speaker} /></span>}
                <span className="text-base leading-relaxed">{interim.text}</span>
              </div>
            </div>
          ))}
          {!isStopping && isRecording && !isPaused && interims.length === 0 && (
            <div className="flex items-center gap-2 mt-4 text-gray-500">
              <div className="w-2 h-2 bg-blue-500 rounded-full animate-pulse" />
              <span className="text-sm">Listening…</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
