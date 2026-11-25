"use client";

import { invoke, isTauri } from "@tauri-apps/api/core";
import { CircleCheck, CloudUpload, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { errorMessage } from "@/services/voiceService";

export function usePublicationDefault() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    let active = true;

    if (isTauri()) {
      void invoke<boolean>("publication_default")
        .then((value) => {
          if (active) {
            setEnabled(value);
          }
        })
        .catch((error) => {
          if (active) {
            setEnabled(false);
            toast.error("Check publishing settings", {
              description: errorMessage(error),
            });
          }
        });
    }

    return () => {
      active = false;
    };
  }, []);

  return { enabled, setEnabled };
}

export function PublicationChoice({
  enabled,
  onChange,
  global = false,
}: {
  enabled: boolean | null;
  onChange: (enabled: boolean) => void;
  global?: boolean;
}) {
  return (
    <div className="space-y-2 rounded-lg border p-4">
      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="publish-to-cica">
          {global
            ? "Publish New Meetings to CICA Meetings Portal"
            : "Publish to CICA Meetings Portal"}
        </Label>
        <Switch
          id="publish-to-cica"
          checked={enabled ?? false}
          disabled={enabled === null}
          onCheckedChange={onChange}
        />
      </div>
      <p className="text-xs text-muted-foreground">
        Audio is uploaded after recording. The website generates the shared
        notes and transcript. Anyone with the CICA site link can read and play
        published meetings. Turn off for a private recording.
      </p>
      {global && (
        <p className="text-xs text-muted-foreground">
          Applies to new meetings. Each recording has its own switch.
        </p>
      )}
    </div>
  );
}

export function PublicationSettings() {
  const { enabled, setEnabled } = usePublicationDefault();
  const [saving, setSaving] = useState(false);

  const save = async (next: boolean) => {
    setSaving(true);

    try {
      await invoke("publication_set_default", { enabled: next });
      setEnabled(next);
    } catch (error) {
      toast.error("Publishing preference needs attention", {
        description: errorMessage(error),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <PublicationChoice
      global
      enabled={saving ? null : enabled}
      onChange={(next) => void save(next)}
    />
  );
}

interface PublicationStatus {
  state:
    | "held"
    | "approved"
    | "queued"
    | "uploading"
    | "processing"
    | "published"
    | "linked"
    | "partial"
    | "failed";
  bytes_sent: number;
  total_bytes: number;
  error: string | null;
  url: string | null;
}

export function PublicationProgress({ meetingId }: { meetingId: string }) {
  const [status, setStatus] = useState<PublicationStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    let reading = false;

    const read = async () => {
      if (reading || !isTauri()) {
        return;
      }

      reading = true;

      try {
        const value = await invoke<PublicationStatus | null>(
          "publication_status",
          { meetingId }
        );

        if (active) {
          setStatus(value);
          setError(null);
        }
      } catch (error_) {
        if (active) {
          setError(errorMessage(error_));
        }
      } finally {
        reading = false;
      }
    };

    void read();
    const timer = setInterval(() => void read(), 3000);

    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [meetingId]);

  if (!status && !error) {
    return null;
  }

  const retry = async () => {
    try {
      await invoke("publication_retry", { meetingId });
      setStatus((current) =>
        current ? { ...current, state: "queued", error: null } : null
      );
    } catch (error_) {
      toast.error("Upload retry needs attention", {
        description: errorMessage(error_),
      });
    }
  };

  const open = async () => {
    try {
      await invoke("publication_open", { meetingId });
    } catch (error_) {
      toast.error("Open the CICA website", {
        description: errorMessage(error_),
      });
    }
  };

  const filed =
    status && ["published", "partial", "linked"].includes(status.state);

  const failed = status?.state === "failed";
  const held = status?.state === "held";

  const labels = {
    held: "Ran over · Publish recording",
    approved: "Queued",
    partial: "Partial transcript · Audio unavailable",
    linked: "Filed",
    published: "Published",
    processing: "Preparing minutes",
    uploading: `Uploading ${Math.round(((status?.bytes_sent ?? 0) / Math.max(status?.total_bytes ?? 0, 1)) * 100)}%`,
    failed: "Retry upload",
    queued: "Queued",
  };

  const label = error || status?.error || (status && labels[status.state]);

  return (
    <div role="status" className="ml-auto shrink-0">
      <Button
        size="sm"
        variant="ghost"
        title={label ?? undefined}
        aria-label={`CICA Meetings Portal · ${label ?? ""}`}
        disabled={!filed && !failed && !held && status?.state !== "processing"}
        onClick={() => void (failed || held ? retry() : open())}
      >
        {filed ? (
          <CircleCheck className="text-success" />
        ) : failed || held || error ? (
          <TriangleAlert />
        ) : (
          <CloudUpload />
        )}
        <span className="hidden sm:inline">CICA Meetings Portal</span>
        {(!filed || status?.state === "partial") && (
          <span>{status ? labels[status.state] : "Status unavailable"}</span>
        )}
      </Button>
    </div>
  );
}
