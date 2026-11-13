"use client";

import { Mic } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import { DeviceSelection } from "@/components/DeviceSelection";
import { useConfig } from "@/contexts/ConfigContext";
import type { SelectedDevices } from "@/contexts/ConfigContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { selectionLabel } from "@/lib/audio-devices";
import { voiceService, errorMessage } from "@/services/voiceService";
import type { RecordingPreferences } from "@/types/voice";

import { SettingsSection } from "./SettingsParts";

/** Settings → Recording: the default mic and call / system audio source, with a live level test. */
export function RecordingTab() {
  const { preferences, setPreferences } = useRecordingPreferences();
  const [saving, setSaving] = useState(false);
  const { isRecording } = useRecordingState();
  const { setSelectedDevices } = useConfig();

  const handleDeviceChange = async (devices: SelectedDevices) => {
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
        description: `Microphone: ${selectionLabel(devices.micDevice)} · System: ${selectionLabel(devices.systemDevice)}`,
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
    <SettingsSection title="Audio sources" icon={<Mic className="h-5 w-5" />}>
      {isRecording && (
        <p
          role="status"
          className="rounded-md border border-warning/30 bg-warning/10 p-2 text-sm text-warning"
        >
          Devices are locked while recording. Stop the current meeting to change
          them.
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
