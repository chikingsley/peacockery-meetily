"use client";

import { useCallback, useMemo, useRef, useState } from "react";
import { toast } from "sonner";

import { useConfig } from "@/contexts/ConfigContext";
import type { FinalTranscriptView } from "@/hooks/meeting-details/useFinalTranscript";
import { activeTranscriptId, speakersOf } from "@/lib/transcript-format";
import { modelLabel } from "@/lib/voice-models";
import { errorMessage } from "@/services/voiceService";
import type {
  MeetingMetadata,
  Transcript,
  TranscriptSource,
} from "@/types/voice";

import { FinalTranscriptBanner } from "./FinalTranscriptBanner";
import { MeetingAudioPlayer } from "./MeetingAudioPlayer";
import { MeetingTranscriptView } from "./MeetingTranscriptView";
import { SpeakerRenameDialog } from "./SpeakerRenameDialog";
import { TranscriptButtonGroup } from "./TranscriptButtonGroup";

interface TranscriptPanelProps {
  meeting: MeetingMetadata;
  transcripts: Transcript[];
  source: TranscriptSource | null;
  /** True right after a stop navigated here (`&source=recording`). */
  justRecorded: boolean;
  finalTranscript: FinalTranscriptView & { isRunning: boolean };
  customPrompt: string;
  onPromptChange: (value: string) => void;
  onToggleSource: () => void;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => void;
  onStartFinalTranscript: (model: string | null) => void;
  onRenameSpeaker: (from: string, to: string) => Promise<void>;
  hasMore: boolean;
  isLoadingMore: boolean;
  totalCount: number;
  onLoadMore: () => void;
}

export function TranscriptPanel({
  meeting,
  transcripts,
  source,
  justRecorded,
  finalTranscript,
  customPrompt,
  onPromptChange,
  onToggleSource,
  onCopyTranscript,
  onOpenMeetingFolder,
  onStartFinalTranscript,
  onRenameSpeaker,
  hasMore,
  isLoadingMore,
  totalCount,
  onLoadMore,
}: TranscriptPanelProps) {
  const { voiceSettings, voiceModels } = useConfig();
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playhead, setPlayhead] = useState<number | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);

  const finalModelLabel = modelLabel(
    voiceModels?.final,
    finalTranscript.model ?? meeting.final_model
  );

  const activeId = useMemo(
    () => activeTranscriptId(transcripts, playhead),
    [transcripts, playhead]
  );

  const audioPath = meeting.status === "recording" ? null : meeting.audio_path;

  const awaitingAutoFinal =
    justRecorded &&
    finalTranscript.state === null &&
    !!voiceSettings?.auto_final;

  const speakerSuggestions = useMemo(() => {
    const names = [
      voiceSettings?.me_label,
      voiceSettings?.them_label,
      ...speakersOf(transcripts),
    ].filter((n): n is string => !!n);

    return [...new Set(names)];
  }, [voiceSettings?.me_label, voiceSettings?.them_label, transcripts]);

  const seek = useCallback((seconds: number) => {
    const audio = audioRef.current;

    if (!audio) {
      return;
    }

    audio.currentTime = seconds;
    setPlayhead(seconds);
    audio
      .play()
      .catch((error) =>
        toast.error("Playback failed", { description: String(error) })
      );
  }, []);

  const rename = useCallback(
    async (from: string, to: string) => {
      try {
        await onRenameSpeaker(from, to);
        toast.success(`Renamed "${from}" to "${to}"`);
      } catch (error) {
        toast.error("Failed to rename speaker", {
          description: errorMessage(error),
        });
        throw error;
      }
    },
    [onRenameSpeaker]
  );

  return (
    <div className="@container relative flex h-full w-full min-w-0 flex-col bg-background">
      <div className="space-y-3 border-b border-border p-4">
        <TranscriptButtonGroup
          transcriptCount={totalCount}
          source={source}
          hasFinal={meeting.has_final}
          hasLive={meeting.has_live}
          finalModelLabel={modelLabel(voiceModels?.final, meeting.final_model)}
          isFinalRunning={finalTranscript.isRunning}
          onToggleSource={onToggleSource}
          onCopyTranscript={onCopyTranscript}
          onOpenMeetingFolder={onOpenMeetingFolder}
          onStartFinalTranscript={onStartFinalTranscript}
        />
        <FinalTranscriptBanner
          view={
            awaitingAutoFinal
              ? {
                  message: null,
                  model: voiceSettings?.final_model ?? null,
                  state: "queued",
                }
              : finalTranscript
          }
          modelLabel={
            awaitingAutoFinal
              ? modelLabel(voiceModels?.final, voiceSettings?.final_model)
              : finalModelLabel
          }
          hasFinal={meeting.has_final}
          onRetry={() => onStartFinalTranscript(finalTranscript.model)}
        />
        {audioPath && (
          <MeetingAudioPlayer
            key={audioPath}
            ref={audioRef}
            audioPath={audioPath}
            onTimeUpdate={setPlayhead}
          />
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        <MeetingTranscriptView
          transcripts={transcripts}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          onLoadMore={onLoadMore}
          onSpeakerClick={setRenaming}
          onSeek={audioPath ? seek : undefined}
          activeId={activeId}
        />
      </div>

      {transcripts.length > 0 && (
        <div className="border-t border-border p-1">
          <textarea
            placeholder="Add context for the summary: people involved, meeting purpose, objective..."
            className="min-h-[80px] w-full resize-y rounded-md border border-border bg-background px-3 py-2 text-sm shadow-sm focus:border-info/30 focus:ring-1 focus:ring-info focus:outline-none"
            value={customPrompt}
            onChange={(e) => onPromptChange(e.target.value)}
          />
        </div>
      )}

      <SpeakerRenameDialog
        speaker={renaming}
        suggestions={speakerSuggestions}
        onClose={() => setRenaming(null)}
        onRename={rename}
      />
    </div>
  );
}
