import { useMemo } from 'react';
import { AlertTriangle, Copy } from 'lucide-react';
import Link from 'next/link';
import { AudioDeviceWarning } from '@/components/AudioDeviceWarning';
import { LiveStatusBadges } from '@/components/LiveStatusBadges';
import { RecordingStatusBar } from '@/components/RecordingStatusBar';
import { Button } from '@/components/ui/button';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useAudioDeviceCheck } from '@/hooks/useAudioDeviceCheck';
import { useAudioLevels } from '@/hooks/useAudioLevels';
import { byChannel, isSystemSilent } from '@/lib/live-transcript';
import { LiveTranscriptView } from './LiveTranscriptView';
import { LiveQuickSettings } from './LiveQuickSettings';

/** Live transcript of the current recording, with the live model, session status and copy. */
export function TranscriptPanel() {
  const { lines, interims, liveStatus, transcriptContainerRef, copyTranscript } = useTranscripts();
  const { isRecording, isPaused, isStopping } = useRecordingState();
  const { voiceSettings } = useConfig();
  const { check, isChecking, hasMicrophone, hasSystemAudio } = useAudioDeviceCheck();
  const levels = useAudioLevels(isRecording);
  const systemSilent = isSystemSilent(levels, isRecording, isPaused);
  const interimLines = useMemo(() => byChannel(interims), [interims]);

  return (
    <div ref={transcriptContainerRef} className="w-full border-r border-gray-200 bg-white flex flex-col overflow-y-auto">
      <div className="sticky top-0 z-20 bg-white px-4 py-3 border-b border-gray-100">
        <div className="flex flex-wrap justify-center items-center gap-3">
          {isRecording && <RecordingStatusBar isPaused={isPaused} />}
          <LiveQuickSettings isRecording={isRecording} />
          {isRecording && <LiveStatusBadges liveStatus={liveStatus} />}
          {lines.length > 0 && (
            <Button variant="outline" size="sm" onClick={copyTranscript} title="Copy transcript">
              <Copy />
              <span className="hidden md:inline">Copy</span>
            </Button>
          )}
        </div>
        {systemSilent && (
          <div role="alert" className="mt-3 mx-auto max-w-[750px] flex items-start gap-2 rounded-md border border-amber-200 bg-amber-50 p-2.5 text-sm text-amber-800">
            <AlertTriangle className="h-4 w-4 mt-0.5 flex-shrink-0" />
            <span>
              No call/system audio detected for {Math.round(levels.system_silent_seconds)} s. Check the system audio
              source in <Link href="/settings?tab=recording" className="underline font-medium">Settings</Link>; for
              Google Voice choose &quot;Google Chrome app audio&quot;.
            </span>
          </div>
        )}
      </div>

      {!isRecording && !isChecking && (
        <div className="flex justify-center px-4 pt-4">
          <AudioDeviceWarning
            hasMicrophone={hasMicrophone}
            hasSystemAudio={hasSystemAudio}
            onRecheck={check}
            isRechecking={isChecking}
          />
        </div>
      )}

      <div className="pb-28">
        <div className="flex justify-center">
          <div className="w-2/3 max-w-[750px]">
            <LiveTranscriptView
              lines={lines}
              interims={interimLines}
              isRecording={isRecording}
              isPaused={isPaused}
              isStopping={isStopping}
              liveEnabled={voiceSettings?.live_enabled ?? true}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
