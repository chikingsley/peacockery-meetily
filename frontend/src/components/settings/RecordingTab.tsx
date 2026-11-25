"use client";

import { Mic } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import { DeviceSelection } from "@/components/DeviceSelection";
import { Button } from "@/components/ui/button";
import { useConfig } from "@/contexts/ConfigContext";
import type { SelectedDevices } from "@/contexts/ConfigContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { useDesktopRuntime } from "@/hooks/useDesktopRuntime";
import { selectionLabel } from "@/lib/audio-devices";
import { voiceService, errorMessage } from "@/services/voiceService";
import type { AudioDevice, RecordingPreferences } from "@/types/voice";

import { SettingsSection, ToggleRow } from "./SettingsParts";

/** Settings → Recording: the default mic and call / system audio source, with a live level test. */
export function RecordingTab() {
  const { preferences, setPreferences } = useRecordingPreferences();
  const [saving, setSaving] = useState(false);
  const { isRecording, isStopping } = useRecordingState();
  const { setSelectedDevices } = useConfig();

  const handleDeviceChange = async (
    devices: SelectedDevices,
    available: AudioDevice[]
  ) => {
    if (!preferences) {
      return;
    }

    const next: RecordingPreferences = {
      ...preferences,
      preferred_mic_device: devices.micDevice,
      preferred_system_device: devices.systemDevice,
    };

    setPreferences(next);
    // The start path reads the in-memory selection, so update it alongside the store.
    setSelectedDevices(devices);
    setSaving(true);

    try {
      await voiceService.setRecordingPreferences(next);
      toast.success("Devices saved", {
        description: `Microphone: ${selectionLabel(devices.micDevice, available)} · System: ${selectionLabel(devices.systemDevice, available)}`,
      });
    } catch (error) {
      toast.error("Failed to save devices", {
        description: errorMessage(error),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-6">
      <SettingsSection title="Audio Devices" icon={<Mic className="h-5 w-5" />}>
        {isRecording && (
          <p
            role="status"
            className="rounded-md border border-warning/30 bg-warning/10 p-2 text-sm text-warning"
          >
            Devices are locked while recording. Stop the current meeting to
            change them.
          </p>
        )}
        {preferences ? (
          <DeviceSelection
            selectedDevices={{
              micDevice: preferences.preferred_mic_device,
              systemDevice: preferences.preferred_system_device,
            }}
            onDeviceChange={handleDeviceChange}
            disabled={saving || isRecording}
          />
        ) : (
          <div className="h-32 animate-pulse rounded-lg bg-muted/40" />
        )}
      </SettingsSection>
      <MeetingFocusSetting locked={isRecording || isStopping} />
    </div>
  );
}

function MeetingFocusSetting({ locked }: { locked: boolean }) {
  const available =
    useDesktopRuntime() && navigator.userAgent.includes("Windows NT");

  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [loadError, setLoadError] = useState("");
  const [saving, setSaving] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!available) {
      return;
    }

    let active = true;
    setLoadError("");
    voiceService
      .meetingFocusEnabled()
      .then((value) => {
        if (active) {
          setEnabled(value);
        }
      })
      .catch((error) => {
        if (active) {
          setLoadError(errorMessage(error));
        }
      });

    return () => {
      active = false;
    };
  }, [attempt, available]);

  async function change(next: boolean) {
    setSaving(true);

    try {
      await voiceService.setMeetingFocus(next);
      setEnabled(next);
    } catch (error) {
      toast.error("Couldn’t change meeting focus", {
        description: errorMessage(error),
      });
      setEnabled(null);
      setAttempt((count) => count + 1);
    } finally {
      setSaving(false);
    }
  }

  if (!available) {
    return null;
  }

  return (
    <SettingsSection title="Meeting Focus">
      {loadError ? (
        <div className="space-y-2">
          <p role="alert" className="text-sm text-muted-foreground">
            Couldn’t read the focus setting. {loadError}
          </p>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setAttempt((count) => count + 1)}
          >
            Retry
          </Button>
        </div>
      ) : enabled === null ? (
        <p role="status" className="text-sm text-muted-foreground">
          Loading setting…
        </p>
      ) : (
        <ToggleRow
          id="meeting-focus"
          label="Quit AI apps while recording"
          description="Meetily quits ChatGPT (Codex), Claude, and Gemini on this PC when recording starts. Running desktop tasks can be interrupted. After recording, Meetily reopens the apps it closed."
          checked={enabled}
          disabled={locked || saving}
          onChange={change}
        />
      )}
    </SettingsSection>
  );
}

/** `get_recording_preferences`, shared by the Recording and General tabs. */
export function useRecordingPreferences() {
  const [preferences, setPreferences] = useState<RecordingPreferences | null>(
    null
  );

  useEffect(() => {
    voiceService
      .getRecordingPreferences()
      .then(setPreferences)
      .catch(async (error) => {
        console.error("get_recording_preferences failed:", error);

        const saveFolder = await voiceService
          .getDefaultRecordingsFolderPath()
          .catch(() => "");

        setPreferences({
          preferred_mic_device: null,
          preferred_system_device: null,
          save_folder: saveFolder,
        });
      });
  }, []);

  return { preferences, setPreferences };
}
