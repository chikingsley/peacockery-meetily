import {
  Upload,
  Loader2,
  AlertCircle,
  X,
  FileAudio,
  Clock,
  HardDrive,
} from "lucide-react";
import { useRouter } from "next/navigation";
import React, { useState, useEffect, useRef } from "react";
import { toast } from "sonner";

import { getAudioFormatsDisplayList } from "@/constants/audioFormats";
import { useConfig } from "@/contexts/ConfigContext";
import { useImportAudio } from "@/hooks/useImportAudio";

import { useSidebar } from "../Sidebar/SidebarProvider";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";

interface ImportAudioDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  preselectedFile?: string | null;
}

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
  }

  return `${minutes}:${secs.toString().padStart(2, "0")}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  if (bytes < 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export function ImportAudioDialog({
  open,
  onOpenChange,
  preselectedFile,
}: ImportAudioDialogProps) {
  const router = useRouter();
  const { refetchMeetings } = useSidebar();
  const { voiceSettings, voiceModels } = useConfig();
  const [title, setTitle] = useState("");
  // "Dialog not yet opened": the init effect must run even when mounted with open=true.
  const prevOpenRef = useRef(false);

  const {
    status,
    fileInfo,
    progress,
    error,
    isImporting,
    selectFile,
    validateFile,
    startImport,
    cancelImport,
    reset,
  } = useImportAudio();

  const isProcessing = isImporting;

  useEffect(() => {
    const wasOpen = prevOpenRef.current;
    prevOpenRef.current = open;

    if (open && !wasOpen) {
      reset();
      setTitle("");

      if (preselectedFile) {
        void validateFile(preselectedFile);
      }
    }
  }, [open, preselectedFile, reset, validateFile]);

  const finalModelLabel = voiceSettings
    ? (voiceModels?.final.find((m) => m.id === voiceSettings.final_model)
        ?.label ?? voiceSettings.final_model)
    : null;

  const handleSelectFile = async () => {
    await selectFile();
  };

  const handleStartImport = async () => {
    if (!fileInfo) {
      return;
    }

    // Resolves on the meeting's `import-complete`; the dialog shows progress until then.
    const meetingId = await startImport(fileInfo.path, title.trim() || null);

    if (!meetingId) {
      return;
    }

    toast.success("Audio imported", {
      description: "The final transcript is running.",
    });
    await refetchMeetings();
    onOpenChange(false);
    router.push(`/meeting-details?id=${encodeURIComponent(meetingId)}`);
  };

  const handleCancel = async () => {
    if (isProcessing) {
      await cancelImport();
      toast.info("Import cancelled");
    }

    onOpenChange(false);
  };

  // Prevent closing during processing
  const handleOpenChange = (newOpen: boolean) => {
    if (!newOpen && isProcessing) {
      return;
    }

    onOpenChange(newOpen);
  };

  const handleEscapeKeyDown = (event: KeyboardEvent) => {
    if (isProcessing) {
      event.preventDefault();
    }
  };

  const handleInteractOutside = (event: Event) => {
    if (isProcessing) {
      event.preventDefault();
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        className="sm:max-w-[500px]"
        onEscapeKeyDown={handleEscapeKeyDown}
        onInteractOutside={handleInteractOutside}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center">
            {isProcessing ? (
              <>
                <Loader2 className="h-5 w-5 animate-spin text-info" />
                Importing Audio...
              </>
            ) : error ? (
              <>
                <AlertCircle className="h-5 w-5 text-destructive" />
                Import Failed
              </>
            ) : (
              <>
                <Upload className="h-5 w-5 text-info" />
                Import Audio File
              </>
            )}
          </DialogTitle>
          <DialogDescription>
            {isProcessing
              ? progress?.message || "Processing audio..."
              : error
                ? "An error occurred during import"
                : "Import an audio file as a new meeting; Peacockery Voice transcribes it"}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* File selection / info */}
          {!isProcessing && !error && (
            <>
              {fileInfo ? (
                <div className="space-y-3 rounded-lg bg-muted/40 p-4">
                  <div className="flex items-start gap-3">
                    <FileAudio className="h-8 w-8 flex-shrink-0 text-info" />
                    <div className="min-w-0 flex-1">
                      <p className="truncate font-medium text-foreground">
                        {fileInfo.filename}
                      </p>
                      <div className="mt-1 flex items-center gap-4 text-sm text-muted-foreground">
                        <span className="flex items-center gap-1">
                          <Clock className="h-3.5 w-3.5" />
                          {formatDuration(fileInfo.duration_seconds)}
                        </span>
                        <span className="flex items-center gap-1">
                          <HardDrive className="h-3.5 w-3.5" />
                          {formatFileSize(fileInfo.size_bytes)}
                        </span>
                        <span className="font-medium text-info">
                          {fileInfo.format}
                        </span>
                      </div>
                    </div>
                  </div>

                  {/* Editable title */}
                  <div className="space-y-1">
                    <label
                      htmlFor="import-title"
                      className="text-sm font-medium text-foreground"
                    >
                      Meeting title (optional)
                    </label>
                    <Input
                      id="import-title"
                      value={title}
                      onChange={(e) => setTitle(e.target.value)}
                      placeholder={fileInfo.filename}
                    />
                  </div>

                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleSelectFile}
                    className="w-full"
                  >
                    Choose Different File
                  </Button>
                </div>
              ) : (
                <div className="rounded-lg border-2 border-dashed border-border p-8 text-center">
                  <FileAudio className="mx-auto mb-4 h-12 w-12 text-muted-foreground" />
                  <Button
                    onClick={handleSelectFile}
                    disabled={status === "validating"}
                  >
                    {status === "validating" ? (
                      <>
                        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                        Validating...
                      </>
                    ) : (
                      <>
                        <Upload className="mr-2 h-4 w-4" />
                        Select Audio File
                      </>
                    )}
                  </Button>
                  <p className="mt-2 text-sm text-muted-foreground">
                    {getAudioFormatsDisplayList()}
                  </p>
                </div>
              )}

              {fileInfo && finalModelLabel && (
                <p className="text-xs text-muted-foreground">
                  Final transcript: {finalModelLabel}
                  {voiceSettings?.final_diarize
                    ? " with speaker labels"
                    : ""}{" "}
                  (change in Settings)
                </p>
              )}
            </>
          )}

          {/* Progress display */}
          {isProcessing && progress && (
            <div className="space-y-2">
              <div className="relative">
                <progress
                  className="h-3 w-full"
                  max={100}
                  value={Math.min(progress.progress_percentage, 100)}
                  aria-label="Import progress"
                />
                <div className="mt-1 flex justify-between text-xs text-muted-foreground">
                  <span>{progress.stage}</span>
                  <span>{Math.round(progress.progress_percentage)}%</span>
                </div>
              </div>
              <p className="text-center text-sm text-muted-foreground">
                {progress.message}
              </p>
            </div>
          )}

          {/* Error display */}
          {error && (
            <div className="rounded-lg border border-destructive/30 bg-destructive/10 p-3">
              <p className="text-sm text-destructive">{error}</p>
            </div>
          )}
        </div>

        <DialogFooter>
          {!isProcessing && !error && (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button onClick={handleStartImport} disabled={!fileInfo}>
                <Upload className="mr-2 h-4 w-4" />
                Import
              </Button>
            </>
          )}
          {isProcessing && (
            <Button variant="outline" onClick={handleCancel}>
              <X className="mr-2 h-4 w-4" />
              Cancel
            </Button>
          )}
          {error && (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Close
              </Button>
              <Button onClick={reset} variant="outline">
                Try Again
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
