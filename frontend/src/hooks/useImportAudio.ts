import { useState, useEffect, useCallback, useRef } from "react";

import { voiceService, errorMessage } from "@/services/voiceService";
import type { AudioFileInfo, ImportProgressEvent } from "@/types/voice";

export type ImportStatus = "idle" | "validating" | "importing" | "error";

/** An import between `start_import_audio_command` and its `import-complete` / `import-error`. */
interface PendingImport {
  /** Null until the start call returns it. */
  meetingId: string | null;
  /** Outcomes that arrived before the meeting id was known: meeting id -> error (null = complete). */
  early: Map<string, string | null>;
  settle: (meetingId: string | null) => void;
}

/**
 * Audio import (§8 "Import"): `start_import_audio_command` returns the new meeting id at once and
 * Rust copies the file in the background, then runs the final transcript with the saved Voice
 * settings. `startImport` resolves with the meeting id on that meeting's `import-complete`, or
 * with null on its `import-error` or a cancel.
 */
export function useImportAudio() {
  const [status, setStatus] = useState<ImportStatus>("idle");
  const [fileInfo, setFileInfo] = useState<AudioFileInfo | null>(null);
  const [progress, setProgress] = useState<ImportProgressEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pendingRef = useRef<PendingImport | null>(null);

  const finish = useCallback(
    (meetingId: string, importError: string | null) => {
      const pending = pendingRef.current;

      if (!pending) {
        return;
      }

      if (pending.meetingId === null) {
        pending.early.set(meetingId, importError);

        return;
      }

      if (pending.meetingId !== meetingId) {
        return;
      }

      pendingRef.current = null;
      setProgress(null);

      if (importError === null) {
        setStatus("idle");
        pending.settle(meetingId);
      } else {
        setStatus("error");
        setError(importError);
        pending.settle(null);
      }
    },
    []
  );

  useEffect(() => {
    let cancelled = false;
    const unlisteners: (() => void)[] = [];

    const keep = (fn: () => void) => {
      if (cancelled) {
        fn();
      } else {
        unlisteners.push(fn);
      }
    };

    (async () => {
      keep(
        await voiceService.onImportProgress((p) => {
          const pending = pendingRef.current;

          if (
            pending &&
            (pending.meetingId === null || pending.meetingId === p.meeting_id)
          ) {
            setProgress(p);
          }
        })
      );
      keep(
        await voiceService.onImportComplete(({ meeting_id }) =>
          finish(meeting_id, null)
        )
      );
      keep(
        await voiceService.onImportError(
          ({ meeting_id, error: importFailure }) =>
            finish(meeting_id, importFailure)
        )
      );
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [finish]);

  const runValidation = useCallback(
    async (validate: () => Promise<AudioFileInfo | null>) => {
      setStatus("validating");
      setError(null);

      try {
        const info = await validate();

        if (info) {
          setFileInfo(info);
        }

        setStatus("idle");

        return info;
      } catch (error_) {
        setStatus("error");
        setError(errorMessage(error_));

        return null;
      }
    },
    []
  );

  const selectFile = useCallback(
    () => runValidation(voiceService.selectAndValidateAudio),
    [runValidation]
  );

  const validateFile = useCallback(
    (path: string) => runValidation(() => voiceService.validateAudioFile(path)),
    [runValidation]
  );

  const startImport = useCallback(
    (sourcePath: string, title: string | null): Promise<string | null> => {
      setStatus("importing");
      setError(null);
      setProgress(null);

      return new Promise((resolve) => {
        const pending: PendingImport = {
          early: new Map(),
          meetingId: null,
          settle: resolve,
        };

        pendingRef.current = pending;
        voiceService
          .startImport(sourcePath, title)
          .then(({ meeting_id }) => {
            if (pendingRef.current !== pending) {
              return;
            }

            pending.meetingId = meeting_id;
            const early = pending.early.get(meeting_id);

            if (early !== undefined) {
              finish(meeting_id, early);
            }
          })
          .catch((error_) => {
            if (pendingRef.current !== pending) {
              return;
            }

            pendingRef.current = null;
            setStatus("error");
            setError(errorMessage(error_));
            resolve(null);
          });
      });
    },
    [finish]
  );

  /** Drop the running import from this view; its later events are ignored. */
  const abandon = useCallback(() => {
    const pending = pendingRef.current;
    pendingRef.current = null;
    pending?.settle(null);
  }, []);

  const cancelImport = useCallback(async () => {
    abandon();

    try {
      await voiceService.cancelImport();
    } catch (error_) {
      console.error("cancel_import_command failed:", error_);
    }

    setStatus("idle");
    setProgress(null);
  }, [abandon]);

  const reset = useCallback(() => {
    abandon();
    setStatus("idle");
    setFileInfo(null);
    setProgress(null);
    setError(null);
  }, [abandon]);

  return {
    cancelImport,
    error,
    fileInfo,
    isImporting: status === "importing",
    progress,
    reset,
    selectFile,
    startImport,
    status,
    validateFile,
  };
}
