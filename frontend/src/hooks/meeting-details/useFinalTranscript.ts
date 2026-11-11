import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { voiceService, errorMessage } from '@/services/voiceService';
import { isFinalInProgress } from '@/lib/live-transcript';
import type { FinalTranscriptState, MeetingMetadata } from '@/types/voice';

export interface FinalTranscriptView {
  state: FinalTranscriptState | null;
  model: string | null;
  message: string | null;
}

/**
 * Final transcript state of one meeting (§6): seeded from metadata and `get_final_transcript_status`,
 * then driven by `final-transcript-status`. `onDone` runs when a run for this meeting finishes.
 */
export function useFinalTranscript(meeting: MeetingMetadata, onDone: () => void | Promise<void>) {
  const [view, setView] = useState<FinalTranscriptView>({
    state: meeting.final_status,
    model: meeting.final_model,
    message: meeting.final_error,
  });
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  useEffect(() => {
    setView({ state: meeting.final_status, model: meeting.final_model, message: meeting.final_error });
  }, [meeting.id, meeting.final_status, meeting.final_model, meeting.final_error]);

  // The run may have moved on between the metadata read and the listener attaching.
  useEffect(() => {
    let cancelled = false;
    voiceService.getFinalTranscriptStatus(meeting.id)
      .then((status) => {
        if (!cancelled && status.state) setView({ state: status.state, model: status.model, message: status.error });
      })
      .catch((error) => console.error('get_final_transcript_status failed:', error));
    return () => { cancelled = true; };
  }, [meeting.id]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    voiceService.onFinalTranscriptStatus((status) => {
      if (status.meeting_id !== meeting.id) return;
      setView({ state: status.state, model: status.model, message: status.message });
      if (status.state === 'done') {
        toast.success('Final transcript ready', {
          description: status.segments_count !== null ? `${status.segments_count} segments` : undefined,
        });
        void onDoneRef.current();
      } else if (status.state === 'failed') {
        toast.error('Final transcript failed', { description: status.message ?? undefined, duration: 10000 });
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [meeting.id]);

  const start = useCallback(async (model: string | null) => {
    try {
      setView({ state: 'queued', model, message: null });
      await voiceService.startFinalTranscript(meeting.id, model);
    } catch (error) {
      const message = errorMessage(error);
      setView({ state: 'failed', model, message });
      toast.error('Could not start the final transcript', { description: message });
    }
  }, [meeting.id]);

  return {
    ...view,
    isRunning: isFinalInProgress(view.state),
    start,
  };
}
