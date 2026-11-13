"use client";

import type { ReactNode } from "react";
import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useMemo,
} from "react";

import { voiceService } from "@/services/voiceService";
import type { KeyStatus, VoiceModels, VoiceSettings } from "@/types/voice";

export interface SelectedDevices {
  micDevice: string | null;
  systemDevice: string | null;
}

interface ConfigContextType {
  /** Rust-owned Peacockery Voice settings; null until loaded. */
  voiceSettings: VoiceSettings | null;
  saveVoiceSettings: (settings: VoiceSettings) => Promise<VoiceSettings>;
  voiceModels: VoiceModels | null;
  refreshVoiceModels: () => Promise<void>;
  keyStatus: KeyStatus | null;
  /** Apply the status a key command returned. */
  setKeyStatus: (status: KeyStatus) => void;
  refreshKeyStatus: () => Promise<KeyStatus | null>;

  selectedDevices: SelectedDevices;
  setSelectedDevices: (devices: SelectedDevices) => void;
}

const ConfigContext = createContext<ConfigContextType | undefined>(undefined);

export function ConfigProvider({ children }: { children: ReactNode }) {
  const [voiceSettings, setVoiceSettings] = useState<VoiceSettings | null>(
    null
  );

  const [voiceModels, setVoiceModels] = useState<VoiceModels | null>(null);
  const [keyStatus, setKeyStatus] = useState<KeyStatus | null>(null);

  const [selectedDevices, setSelectedDevices] = useState<SelectedDevices>({
    micDevice: null,
    systemDevice: null,
  });

  const refreshVoiceModels = useCallback(async () => {
    try {
      setVoiceModels(await voiceService.listModels());
    } catch (error) {
      console.error("[ConfigContext] voice_list_models failed:", error);
    }
  }, []);

  const refreshKeyStatus = useCallback(async () => {
    try {
      const status = await voiceService.getKeyStatus();
      setKeyStatus(status);

      return status;
    } catch (error) {
      console.error("[ConfigContext] voice_get_key_status failed:", error);

      return null;
    }
  }, []);

  const saveVoiceSettings = useCallback(async (settings: VoiceSettings) => {
    const saved = await voiceService.saveSettings(settings);
    setVoiceSettings(saved);

    return saved;
  }, []);

  useEffect(() => {
    voiceService
      .getSettings()
      .then(setVoiceSettings)
      .catch((error) =>
        console.error("[ConfigContext] voice_get_settings failed:", error)
      );
    void refreshKeyStatus();
    void refreshVoiceModels();

    voiceService
      .getRecordingPreferences()
      .then((prefs) =>
        setSelectedDevices({
          micDevice: prefs.preferred_mic_device,
          systemDevice: prefs.preferred_system_device,
        })
      )
      .catch((error) =>
        console.error(
          "[ConfigContext] get_recording_preferences failed:",
          error
        )
      );

    let unlisten: (() => void) | undefined;
    let cancelled = false;
    voiceService
      .onVoiceSettingsUpdated(setVoiceSettings)
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((error) =>
        console.error("Native event synchronization failed", error)
      );

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refreshKeyStatus, refreshVoiceModels]);

  const value = useMemo<ConfigContextType>(
    () => ({
      keyStatus,
      refreshKeyStatus,
      refreshVoiceModels,
      saveVoiceSettings,
      selectedDevices,
      setKeyStatus,
      setSelectedDevices,
      voiceModels,
      voiceSettings,
    }),
    [
      voiceSettings,
      saveVoiceSettings,
      voiceModels,
      refreshVoiceModels,
      keyStatus,
      refreshKeyStatus,
      selectedDevices,
    ]
  );

  return (
    <ConfigContext.Provider value={value}>{children}</ConfigContext.Provider>
  );
}

export function useConfig() {
  const context = useContext(ConfigContext);

  if (context === undefined) {
    throw new Error("useConfig must be used within a ConfigProvider");
  }

  return context;
}
