'use client';

import { AlertCircle, Loader2, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { FINAL_STATE_TEXT, isFinalInProgress } from '@/lib/live-transcript';
import type { FinalTranscriptView } from '@/hooks/meeting-details/useFinalTranscript';

/** Progress or failure of the meeting's final transcript run (`final-transcript-status`). */
export function FinalTranscriptBanner({ view, modelLabel, hasFinal, onRetry }: {
  view: FinalTranscriptView;
  modelLabel: string | null;
  hasFinal: boolean;
  onRetry: () => void;
}) {
  if (isFinalInProgress(view.state)) {
    return (
      <div role="status" className="flex items-center gap-2 rounded-md border border-blue-200 bg-blue-50 px-3 py-2 text-sm text-blue-800">
        <Loader2 className="h-4 w-4 animate-spin flex-shrink-0" />
        <span>{FINAL_STATE_TEXT[view.state!]}{modelLabel ? ` · ${modelLabel}` : ''}</span>
      </div>
    );
  }
  if (view.state === 'failed') {
    return (
      <div role="alert" className="flex items-start gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800">
        <AlertCircle className="h-4 w-4 mt-0.5 flex-shrink-0" />
        <div className="flex-1 min-w-0">
          <p className="font-medium">{FINAL_STATE_TEXT.failed}{modelLabel ? ` · ${modelLabel}` : ''}</p>
          {view.message && <p className="text-xs mt-0.5 break-words">{view.message}</p>}
        </div>
        <Button size="sm" variant="outline" className="h-7 border-red-200 bg-white" onClick={onRetry}>
          <RotateCcw className="h-3.5 w-3.5 mr-1" /> Retry
        </Button>
      </div>
    );
  }
  if (!hasFinal && view.state === null) {
    return (
      <p className="text-center text-xs text-gray-500">
        No final transcript yet. Run it for the accurate text and speaker labels.
      </p>
    );
  }
  return null;
}
