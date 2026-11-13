"use client";

import { Copy, FolderOpen, Loader2, RefreshCw } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { ButtonGroup } from "@/components/ui/button-group";
import type { TranscriptSource } from "@/types/voice";

import { FinalTranscriptDialog } from "./FinalTranscriptDialog";

interface TranscriptButtonGroupProps {
  transcriptCount: number;
  /** Source of the rows on screen. */
  source: TranscriptSource | null;
  hasFinal: boolean;
  hasLive: boolean;
  finalModelLabel: string | null;
  isFinalRunning: boolean;
  onToggleSource: () => void;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => void;
  onStartFinalTranscript: (model: string | null) => void;
}

export function TranscriptButtonGroup({
  transcriptCount,
  source,
  hasFinal,
  hasLive,
  finalModelLabel,
  isFinalRunning,
  onToggleSource,
  onCopyTranscript,
  onOpenMeetingFolder,
  onStartFinalTranscript,
}: TranscriptButtonGroupProps) {
  const [showDialog, setShowDialog] = useState(false);

  return (
    <div className="flex w-full flex-col items-center gap-2">
      <ButtonGroup>
        <Button
          variant="outline"
          size="sm"
          onClick={onCopyTranscript}
          disabled={transcriptCount === 0}
          title={
            transcriptCount === 0
              ? "No transcript available"
              : "Copy transcript"
          }
        >
          <Copy />
          <span className="hidden @[22rem]:inline">Copy</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          onClick={onOpenMeetingFolder}
          title="Open the recording folder"
        >
          <FolderOpen className="@[22rem]:mr-2" size={18} />
          <span className="hidden @[22rem]:inline">Folder</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          onClick={() => setShowDialog(true)}
          disabled={isFinalRunning}
          title={
            hasFinal
              ? "Re-run the final transcript"
              : "Run the final transcript"
          }
        >
          {isFinalRunning ? (
            <Loader2 className="animate-spin @[22rem]:mr-2" size={18} />
          ) : (
            <RefreshCw className="@[22rem]:mr-2" size={18} />
          )}
          <span className="hidden @[22rem]:inline">
            {hasFinal ? "Re-run final transcript" : "Run final transcript"}
          </span>
        </Button>
      </ButtonGroup>

      <div className="flex items-center gap-2 text-xs">
        {source === "final" && (
          <span className="rounded-full bg-success/10 px-2 py-0.5 font-medium text-success">
            Final{finalModelLabel ? ` · ${finalModelLabel}` : ""}
          </span>
        )}
        {source === "live" && (
          <span className="rounded-full bg-muted/40 px-2 py-0.5 font-medium text-foreground">
            Live draft
          </span>
        )}
        {hasFinal && hasLive && (
          <button
            type="button"
            onClick={onToggleSource}
            className="text-info hover:underline"
          >
            {source === "live" ? "Show final transcript" : "View live draft"}
          </button>
        )}
      </div>

      <FinalTranscriptDialog
        open={showDialog}
        onOpenChange={setShowDialog}
        hasFinal={hasFinal}
        onStart={onStartFinalTranscript}
      />
    </div>
  );
}
