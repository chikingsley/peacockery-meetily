'use client';

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Mic } from 'lucide-react';
import { DeviceSelection } from '@/components/DeviceSelection';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig, type SelectedDevices } from '@/contexts/ConfigContext';
import { voiceService, errorMessage } from '@/services/voiceService';
import { selectionLabel } from '@/lib/audio-devices';
import type { RecordingPreferences } from '@/types/voice';
import { SettingsSection } from './SettingsParts';

/** Settings → Recording: the default mic and call / system audio source, with a live level test. */
export function RecordingTab() {
  const { preferences, setPreferences } = useRecordingPreferences();
  const [saving, setSaving] = useState(false);
  const { isRecording } = useRecordingState();
  const { setSelectedDevices } = useConfig();

  const handleDeviceChange = async (devices: SelectedDevices) => {
    if (!preferences) return;
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
      toast.success('Devices saved', {
        description: `Microphone: ${selectionLabel(devices.micDevice)} · System: ${selectionLabel(devices.systemDevice)}`,
      });
    } catch (error) {
      toast.error('Failed to save devices', { description: errorMessage(error) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <SettingsSection title="Audio sources" icon={<Mic className="w-5 h-5" />}>
      {isRecording && (
        <p role="status" className="text-sm text-amber-700 bg-amber-50 border border-amber-200 rounded-md p-2">
          Devices are locked while recording. Stop the current meeting to change them.
        </p>
      )}
      {preferences ? (
        <DeviceSelection
          selectedDevices={{ micDevice: preferences.preferred_mic_device, systemDevice: preferences.preferred_system_device }}
          onDeviceChange={handleDeviceChange}
          disabled={saving || isRecording}
        />
      ) : (
        <div className="animate-pulse h-32 bg-gray-100 rounded-lg" />
      )}
    </SettingsSection>
  );
}

/** `get_recording_preferences`, shared by the Recording and General tabs. */
export function useRecordingPreferences() {
  const [preferences, setPreferences] = useState<RecordingPreferences | null>(null);
  useEffect(() => {
    voiceService.getRecordingPreferences()
      .then(setPreferences)
      .catch(async (error) => {
        console.error('get_recording_preferences failed:', error);
        const saveFolder = await voiceService.getDefaultRecordingsFolderPath().catch(() => '');
        setPreferences({ save_folder: saveFolder, preferred_mic_device: null, preferred_system_device: null });
      });
  }, []);
  return { preferences, setPreferences };
}
