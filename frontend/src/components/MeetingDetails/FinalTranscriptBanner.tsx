"use client";

import { AlertCircle, Loader2, RotateCcw } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { FinalTranscriptView } from "@/hooks/meeting-details/useFinalTranscript";
import { FINAL_STATE_TEXT, isFinalInProgress } from "@/lib/live-transcript";

/** Progress or failure of the meeting's final transcript run (`final-transcript-status`). */
export function FinalTranscriptBanner({
  view,
  modelLabel,
  hasFinal,
  onRetry,
}: {
  view: FinalTranscriptView;
  modelLabel: string | null;
  hasFinal: boolean;
  onRetry: () => void;
}) {
  if (view.state && isFinalInProgress(view.state)) {
    return (
      <div
        role="status"
        className="flex items-center gap-2 rounded-md border border-info/30 bg-info/10 px-3 py-2 text-sm text-info"
      >
        <Loader2 className="h-4 w-4 flex-shrink-0 animate-spin" />
        <span>
          {FINAL_STATE_TEXT[view.state]}
          {modelLabel ? ` · ${modelLabel}` : ""}
        </span>
      </div>
    );
  }

  if (view.state === "failed") {
    return (
      <div
        role="alert"
        className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive"
      >
        <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0" />
        <div className="min-w-0 flex-1">
          <p className="font-medium">
            {FINAL_STATE_TEXT.failed}
            {modelLabel ? ` · ${modelLabel}` : ""}
          </p>
          {view.message && (
            <p className="mt-0.5 text-xs break-words">{view.message}</p>
          )}
        </div>
        <Button size="sm" variant="outline" className="h-7" onClick={onRetry}>
          <RotateCcw className="mr-1 h-3.5 w-3.5" /> Retry
        </Button>
      </div>
    );
  }

  if (!hasFinal && view.state === null) {
    return (
      <p className="text-center text-xs text-muted-foreground">
        No final transcript yet. Run it for the accurate text and speaker
        labels.
      </p>
    );
  }

  return null;
}
