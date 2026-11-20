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
  /** Re-read the settings after a command that changed them (activation). */
  reloadVoiceSettings: () => Promise<void>;
  voiceModels: VoiceModels | null;
  refreshVoiceModels: () => Promise<void>;
  keyStatus: KeyStatus | null;
  refreshKeyStatus: () => Promise<KeyStatus | null>;

  selectedDevices: SelectedDevices;
  /** The saved device preferences have been read (or failed to load). */
  devicesReady: boolean;
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

  const [devicesReady, setDevicesReady] = useState(false);

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

  const reloadVoiceSettings = useCallback(async () => {
    setVoiceSettings(await voiceService.getSettings());
  }, []);

  useEffect(() => {
    reloadVoiceSettings().catch((error) =>
      console.error("[ConfigContext] voice_get_settings failed:", error)
    );
    void refreshKeyStatus();
    void refreshVoiceModels();

    voiceService
      .getRecordingPreferences()
      .then((prefs) => {
        setSelectedDevices({
          micDevice: prefs.preferred_mic_device,
          systemDevice: prefs.preferred_system_device,
        });
        setDevicesReady(true);
      })
      .catch((error) => {
        console.error(
          "[ConfigContext] get_recording_preferences failed:",
          error
        );
        setDevicesReady(true);
      });

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
  }, [refreshKeyStatus, refreshVoiceModels, reloadVoiceSettings]);

  const value = useMemo<ConfigContextType>(
    () => ({
      keyStatus,
      refreshKeyStatus,
      refreshVoiceModels,
      reloadVoiceSettings,
      saveVoiceSettings,
      selectedDevices,
      devicesReady,
      setSelectedDevices,
      voiceModels,
      voiceSettings,
    }),
    [
      voiceSettings,
      saveVoiceSettings,
      reloadVoiceSettings,
      voiceModels,
      refreshVoiceModels,
      keyStatus,
      refreshKeyStatus,
      selectedDevices,
      devicesReady,
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
