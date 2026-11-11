"use client";

import { useCallback, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';
import type { MeetingMetadata, Transcript, TranscriptSource } from '@/types/voice';
import type { FinalTranscriptView } from '@/hooks/meeting-details/useFinalTranscript';
import { useConfig } from '@/contexts/ConfigContext';
import { errorMessage } from '@/services/voiceService';
import { activeTranscriptId, speakersOf } from '@/lib/transcript-format';
import { modelLabel } from '@/lib/voice-models';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { FinalTranscriptBanner } from './FinalTranscriptBanner';
import { MeetingTranscriptView } from './MeetingTranscriptView';
import { MeetingAudioPlayer } from './MeetingAudioPlayer';
import { SpeakerRenameDialog } from './SpeakerRenameDialog';

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

  const finalModelLabel = modelLabel(voiceModels?.final, finalTranscript.model ?? meeting.final_model);
  const activeId = useMemo(() => activeTranscriptId(transcripts, playhead), [transcripts, playhead]);
  const audioPath = meeting.status === 'recording' ? null : meeting.audio_path;
  const awaitingAutoFinal = justRecorded && finalTranscript.state === null && !!voiceSettings?.auto_final;

  const speakerSuggestions = useMemo(() => {
    const names = [voiceSettings?.me_label, voiceSettings?.them_label, ...speakersOf(transcripts)]
      .filter((n): n is string => !!n);
    return [...new Set(names)];
  }, [voiceSettings?.me_label, voiceSettings?.them_label, transcripts]);

  const seek = useCallback((seconds: number) => {
    const audio = audioRef.current;
    if (!audio) return;
    audio.currentTime = seconds;
    setPlayhead(seconds);
    audio.play().catch(() => {});
  }, []);

  const rename = useCallback(async (from: string, to: string) => {
    try {
      await onRenameSpeaker(from, to);
      toast.success(`Renamed "${from}" to "${to}"`);
    } catch (error) {
      toast.error('Failed to rename speaker', { description: errorMessage(error) });
      throw error;
    }
  }, [onRenameSpeaker]);

  return (
    <div className="flex h-full min-w-0 w-full bg-white flex-col relative @container">
      <div className="p-4 border-b border-gray-200 space-y-3">
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
          view={awaitingAutoFinal ? { state: 'queued', model: voiceSettings?.final_model ?? null, message: null } : finalTranscript}
          modelLabel={awaitingAutoFinal ? modelLabel(voiceModels?.final, voiceSettings?.final_model) : finalModelLabel}
          hasFinal={meeting.has_final}
          onRetry={() => onStartFinalTranscript(finalTranscript.model)}
        />
        {audioPath && (
          <MeetingAudioPlayer key={audioPath} ref={audioRef} audioPath={audioPath} onTimeUpdate={setPlayhead} />
        )}
      </div>

      <div className="flex-1 min-h-0 overflow-hidden">
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
        <div className="p-1 border-t border-gray-200">
          <textarea
            placeholder="Add context for the summary: people involved, meeting purpose, objective..."
            className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[80px] resize-y"
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
