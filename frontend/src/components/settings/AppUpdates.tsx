"use client";

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { errorMessage } from "@/services/voiceService";

import { SettingsSection } from "./SettingsParts";

interface Release {
  version: string;
  notes: string;
}

interface Progress {
  downloaded: number;
  total: number | null;
  installing: boolean;
}

export function UpdateNotice() {
  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    let active = true;
    void invoke<Release | null>("check_app_update")
      .then((release) => {
        if (active && release) {
          toast.info(`Meetily ${release.version} is available`, {
            duration: 15_000,
            action: {
              label: "View update",
              onClick: () => {
                window.location.href = "/settings?tab=general";
              },
            },
          });
        }
      })
      .catch(() => {
        /* Settings offers an explicit retry and error details. */
      });

    return () => {
      active = false;
    };
  }, []);

  return null;
}

export function AppUpdates() {
  const { isRecording } = useRecordingState();
  const [release, setRelease] = useState<Release | null>(null);

  const [message, setMessage] = useState(
    "Check for a newer version of Meetily."
  );

  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);

  const check = async () => {
    setBusy(true);
    setProgress(null);

    try {
      const next = await invoke<Release | null>("check_app_update");
      setRelease(next);
      setMessage(
        next
          ? `Version ${next.version} is ready to download.`
          : "You’re up to date."
      );
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    if (!release) {
      return;
    }

    setBusy(true);
    const channel = new Channel<Progress>((event) => setProgress(event));

    try {
      setMessage("Downloading and verifying the update…");
      await invoke("install_app_update", {
        version: release.version,
        onProgress: channel,
      });
    } catch (error) {
      setMessage(errorMessage(error));
      setProgress(null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingsSection
      title="App updates"
      description="Install the latest release and restart. Your recordings and settings stay on this PC."
    >
      <p className="text-sm" role="status">
        {progress?.installing
          ? "Opening the installer. Meetily will restart after installation."
          : message}
      </p>
      {release?.notes && (
        <p className="text-sm whitespace-pre-wrap text-muted-foreground">
          {release.notes}
        </p>
      )}
      {progress && (
        <progress
          className="w-full"
          aria-label="Update download"
          max={progress.total ?? undefined}
          value={progress.total ? progress.downloaded : undefined}
        />
      )}
      {isRecording && (
        <p className="text-sm text-muted-foreground">
          Finish recording to install an update.
        </p>
      )}
      <div className="flex gap-2">
        <Button variant="outline" disabled={busy} onClick={() => void check()}>
          Check for updates
        </Button>
        {release && (
          <Button disabled={busy || isRecording} onClick={() => void install()}>
            Install and restart
          </Button>
        )}
      </div>
    </SettingsSection>
  );
}
