'use client';

import { useCallback, useState } from 'react';
import { Mic, Pause, Play, Square } from 'lucide-react';
import { toast } from 'sonner';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { useRecordingState, STOPPING_MESSAGE } from '@/contexts/RecordingStateContext';
import { useRecordingStop } from '@/contexts/RecordingStopContext';
import { voiceService, errorMessage } from '@/services/voiceService';
import { useAudioLevels } from '@/hooks/useAudioLevels';
import { SYSTEM_SILENCE_WARN_SECONDS } from '@/lib/live-transcript';

interface RecordingControlsProps {
  onStart: () => Promise<void>;
}

/** Start / pause / resume / stop, with live mic and call levels from `audio-levels`. */
export function RecordingControls({ onStart }: RecordingControlsProps) {
  const { isRecording, isPaused, isStartingRecording, isStopping } = useRecordingState();
  const { stopRecording } = useRecordingStop();
  const [isToggling, setIsToggling] = useState(false);
  const levels = useAudioLevels(isRecording);

  const togglePause = useCallback(async () => {
    setIsToggling(true);
    try {
      if (isPaused) await voiceService.resumeRecording();
      else await voiceService.pauseRecording();
    } catch (error) {
      toast.error(isPaused ? 'Failed to resume recording' : 'Failed to pause recording', {
        description: errorMessage(error),
      });
    } finally {
      setIsToggling(false);
    }
  }, [isPaused]);

  const busy = isStartingRecording || isToggling;

  return (
    <TooltipProvider>
      <div className="flex items-center space-x-2 bg-white rounded-full shadow-lg px-4 py-2">
        {isStopping ? (
          <div className="flex items-center gap-3 pr-2" role="status" aria-live="polite">
            <button
              disabled
              aria-label="Stopping recording"
              className="w-10 h-10 flex items-center justify-center bg-gray-400 rounded-full text-white cursor-not-allowed"
            >
              <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-white" />
            </button>
            <span className="text-sm font-medium text-gray-700 whitespace-nowrap">{STOPPING_MESSAGE}</span>
          </div>
        ) : !isRecording ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => void onStart()}
                disabled={busy}
                className={`w-12 h-12 flex items-center justify-center ${busy ? 'bg-gray-400' : 'bg-red-500 hover:bg-red-600'} rounded-full text-white transition-colors`}
              >
                {isStartingRecording
                  ? <div className="animate-spin rounded-full h-5 w-5 border-b-2 border-white" />
                  : <Mic size={20} />}
              </button>
            </TooltipTrigger>
            <TooltipContent><p>Start recording</p></TooltipContent>
          </Tooltip>
        ) : (
          <>
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  onClick={() => void togglePause()}
                  disabled={busy}
                  className={`w-10 h-10 flex items-center justify-center border-2 rounded-full transition-colors ${busy
                    ? 'bg-gray-200 border-gray-300 text-gray-400'
                    : 'bg-white border-gray-300 text-gray-600 hover:border-gray-400 hover:bg-gray-50'}`}
                >
                  {isPaused ? <Play size={16} /> : <Pause size={16} />}
                </button>
              </TooltipTrigger>
              <TooltipContent><p>{isPaused ? 'Resume recording' : 'Pause recording'}</p></TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  onClick={() => void stopRecording()}
                  disabled={busy}
                  className={`w-10 h-10 flex items-center justify-center ${busy ? 'bg-gray-400' : 'bg-red-500 hover:bg-red-600'} rounded-full text-white transition-colors`}
                >
                  <Square size={16} />
                </button>
              </TooltipTrigger>
              <TooltipContent><p>Stop recording</p></TooltipContent>
            </Tooltip>
          </>
        )}

        {!isStopping && (
          <>
            <LevelBar label="Mic" level={isRecording && !isPaused ? levels.mic : 0} paused={isPaused} />
            <LevelBar
              label="System"
              level={isRecording && !isPaused ? levels.system : 0}
              paused={isPaused}
              warn={isRecording && !isPaused && levels.system_silent_seconds > SYSTEM_SILENCE_WARN_SECONDS}
              title={levels.system_silent_seconds > SYSTEM_SILENCE_WARN_SECONDS ? `No system audio for ${Math.round(levels.system_silent_seconds)} s` : undefined}
            />
          </>
        )}
      </div>
    </TooltipProvider>
  );
}

function LevelBar({ label, level, paused, warn = false, title }: {
  label: string;
  level: number;
  paused: boolean;
  warn?: boolean;
  title?: string;
}) {
  const width = `${Math.round(Math.min(1, Math.max(0, level)) * 100)}%`;
  return (
    <div className="flex items-center gap-1.5 ml-2" title={title}>
      <span className={`text-xs ${warn ? 'text-amber-600 font-semibold' : 'text-gray-500'}`}>{label}</span>
      <div className="w-16 h-1.5 bg-gray-200 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full transition-[width] duration-100 ${paused ? 'bg-orange-400' : 'bg-red-500'}`}
          style={{ width }}
        />
      </div>
    </div>
  );
}
