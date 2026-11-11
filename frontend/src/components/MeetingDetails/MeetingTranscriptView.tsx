'use client';

import { memo, useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { Transcript } from '@/types/voice';
import { SpeakerChip } from '@/components/SpeakerChip';
import { formatRecordingTime, groupBySpeaker, type SpeakerGroup } from '@/lib/transcript-format';

interface MeetingTranscriptViewProps {
  transcripts: Transcript[];
  hasMore: boolean;
  isLoadingMore: boolean;
  totalCount: number;
  onLoadMore: () => void;
  /** Click on a speaker chip (rename). */
  onSpeakerClick: (speaker: string) => void;
  /** Click on a line (seek the audio player); absent when there is no audio. */
  onSeek?: (seconds: number) => void;
  /** Id of the line under the audio playhead. */
  activeId: string | null;
}

const Group = memo(function Group({ group, onSpeakerClick, onSeek, activeId }: {
  group: SpeakerGroup<Transcript>;
  onSpeakerClick: (speaker: string) => void;
  onSeek?: (seconds: number) => void;
  activeId: string | null;
}) {
  return (
    <div className="pb-4">
      <div className="flex items-center gap-2 mb-1">
        {group.speaker
          ? <SpeakerChip speaker={group.speaker} onClick={() => onSpeakerClick(group.speaker!)} title="Rename this speaker" />
          : <span className="text-xs font-medium text-gray-400">No speaker</span>}
        <span className="text-xs text-gray-400 tabular-nums">{formatRecordingTime(group.items[0].audio_start_time)}</span>
      </div>
      <div className="space-y-0.5">
        {group.items.map(t => {
          const seekable = !!onSeek && t.audio_start_time !== null;
          return (
            <p
              key={t.id}
              id={`transcript-${t.id}`}
              onClick={seekable ? () => onSeek!(t.audio_start_time!) : undefined}
              title={seekable ? `Play from ${formatRecordingTime(t.audio_start_time)}` : undefined}
              className={`rounded px-1 -mx-1 text-[15px] leading-relaxed text-gray-800 ${seekable ? 'cursor-pointer hover:bg-gray-50' : ''} ${activeId === t.id ? 'bg-blue-50' : ''}`}
            >
              {t.text.trim() || '[Silence]'}
            </p>
          );
        })}
      </div>
    </div>
  );
});

/** A meeting's stored transcript, grouped by consecutive speaker, virtualized with infinite scroll. */
export function MeetingTranscriptView({
  transcripts,
  hasMore,
  isLoadingMore,
  totalCount,
  onLoadMore,
  onSpeakerClick,
  onSeek,
  activeId,
}: MeetingTranscriptViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const groups = useMemo(() => groupBySpeaker(transcripts), [transcripts]);

  const virtualizer = useVirtualizer({
    count: groups.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 96,
    overscan: 6,
    getItemKey: (index) => groups[index].key,
  });

  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore || isLoadingMore) return;
    const observer = new IntersectionObserver(
      (entries) => { if (entries[0].isIntersecting) onLoadMore(); },
      { root: scrollRef.current, rootMargin: '200px' },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [hasMore, isLoadingMore, onLoadMore, groups.length]);

  if (transcripts.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-center text-sm text-gray-500">
        No transcript lines yet.
      </div>
    );
  }

  return (
    <div ref={scrollRef} className="h-full overflow-y-auto px-4 py-3">
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative', width: '100%' }}>
        {virtualizer.getVirtualItems().map(row => (
          <div
            key={row.key}
            data-index={row.index}
            ref={virtualizer.measureElement}
            style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${row.start}px)` }}
          >
            <Group group={groups[row.index]} onSpeakerClick={onSpeakerClick} onSeek={onSeek} activeId={activeId} />
          </div>
        ))}
      </div>
      {(hasMore || isLoadingMore) && (
        <div ref={sentinelRef} className="flex justify-center py-4 text-sm text-gray-400">
          {isLoadingMore ? 'Loading more…' : `Showing ${transcripts.length} of ${totalCount} lines`}
        </div>
      )}
    </div>
  );
}
