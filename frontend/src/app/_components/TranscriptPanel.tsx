import { AlertTriangle, Copy } from "lucide-react";
import Link from "next/link";
import { useMemo } from "react";

import { AudioDeviceWarning } from "@/components/AudioDeviceWarning";
import { LiveStatusBadges } from "@/components/LiveStatusBadges";
import { RecordingStatusBar } from "@/components/RecordingStatusBar";
import { Button } from "@/components/ui/button";
import { useConfig } from "@/contexts/ConfigContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { useTranscripts } from "@/contexts/TranscriptContext";
import { useAudioDeviceCheck } from "@/hooks/useAudioDeviceCheck";
import { useAudioLevels } from "@/hooks/useAudioLevels";
import { byChannel, isSystemSilent } from "@/lib/live-transcript";

import { LiveQuickSettings } from "./LiveQuickSettings";
import { LiveTranscriptView } from "./LiveTranscriptView";

/** Live transcript of the current recording, with the live model, session status and copy. */
export function TranscriptPanel() {
  const {
    lines,
    interims,
    liveStatus,
    transcriptContainerRef,
    copyTranscript,
  } = useTranscripts();

  const { isRecording, isPaused, isStopping } = useRecordingState();
  const { voiceSettings } = useConfig();

  const { check, isChecking, hasMicrophone, hasSystemAudio } =
    useAudioDeviceCheck();

  const levels = useAudioLevels(isRecording);
  const systemSilent = isSystemSilent(levels, isRecording, isPaused);
  const interimLines = useMemo(() => byChannel(interims), [interims]);

  return (
    <div
      ref={transcriptContainerRef}
      className="flex w-full flex-col overflow-y-auto border-r border-border bg-background"
    >
      <div className="sticky top-0 z-20 border-b border-border bg-background px-4 py-3">
        <div className="flex flex-wrap items-center justify-center gap-3">
          {isRecording && <RecordingStatusBar isPaused={isPaused} />}
          <LiveQuickSettings isRecording={isRecording} />
          {isRecording && <LiveStatusBadges liveStatus={liveStatus} />}
          {lines.length > 0 && (
            <Button
              variant="outline"
              size="sm"
              onClick={copyTranscript}
              title="Copy transcript"
            >
              <Copy />
              <span className="hidden md:inline">Copy</span>
            </Button>
          )}
        </div>
        {systemSilent && (
          <div
            role="alert"
            className="mx-auto mt-3 flex max-w-[750px] items-start gap-2 rounded-md border border-warning/30 bg-warning/10 p-2.5 text-sm text-warning"
          >
            <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0" />
            <span>
              No call/system audio detected for{" "}
              {Math.round(levels.system_silent_seconds)} s. Check the system
              audio source in{" "}
              <Link
                href="/settings?tab=recording"
                className="font-medium underline"
              >
                Settings
              </Link>
              ; for Google Voice choose &quot;Google Chrome app audio&quot;.
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
