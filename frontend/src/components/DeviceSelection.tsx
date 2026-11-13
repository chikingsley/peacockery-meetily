import { isTauri } from "@tauri-apps/api/core";
import { RefreshCw, Mic, Speaker } from "lucide-react";
import React, { useState, useEffect, useRef, useCallback } from "react";

import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { SelectedDevices } from "@/contexts/ConfigContext";
import { useDesktopRuntime } from "@/hooks/useDesktopRuntime";
import {
  deviceLabel,
  deviceValue,
  inputDevices as inputsOf,
  systemDevices as systemsOf,
} from "@/lib/audio-devices";
import { voiceService } from "@/services/voiceService";
import type { AudioDevice, AudioLevels } from "@/types/voice";

interface DeviceSelectionProps {
  selectedDevices: SelectedDevices;
  onDeviceChange: (devices: SelectedDevices) => void;
  disabled?: boolean;
}

export function DeviceSelection({
  selectedDevices,
  onDeviceChange,
  disabled = false,
}: DeviceSelectionProps) {
  const desktop = useDesktopRuntime();
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [levels, setLevels] = useState<AudioLevels | null>(null);

  const inputDevices = React.useMemo(() => inputsOf(devices), [devices]);
  const outputDevices = React.useMemo(() => systemsOf(devices), [devices]);

  const fetchDevices = useCallback(async () => {
    if (!isTauri()) {
      setLoading(false);
      setRefreshing(false);

      return;
    }

    try {
      setError(null);
      setDevices(await voiceService.getAudioDevices());
    } catch (error_) {
      console.error("get_audio_devices failed:", error_);
      setError("Failed to load audio devices. Check Windows sound settings.");
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void fetchDevices();
  }, [fetchDevices]);

  // Reset a selection whose device is gone (e.g. the Jabra headset is off) back to the default.
  const onDeviceChangeRef = useRef(onDeviceChange);
  useEffect(() => {
    onDeviceChangeRef.current = onDeviceChange;
  }, [onDeviceChange]);
  useEffect(() => {
    if (loading || disabled) {
      return;
    }

    let next = selectedDevices;

    if (
      selectedDevices.micDevice &&
      inputDevices.length > 0 &&
      !inputDevices.some((d) => deviceValue(d) === selectedDevices.micDevice)
    ) {
      next = { ...next, micDevice: null };
    }

    if (
      selectedDevices.systemDevice &&
      outputDevices.length > 0 &&
      !outputDevices.some(
        (d) => deviceValue(d) === selectedDevices.systemDevice
      )
    ) {
      next = { ...next, systemDevice: null };
    }

    if (next !== selectedDevices) {
      onDeviceChangeRef.current(next);
    }
  }, [inputDevices, outputDevices, loading, disabled, selectedDevices]);

  // Device test: real RMS levels while monitoring.
  useEffect(() => {
    if (!isTesting) {
      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | undefined;
    voiceService
      .onAudioLevels(setLevels)
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((error_) =>
        console.error("Native event synchronization failed", error_)
      );

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [isTesting]);

  // A device change during the test restarts monitoring on the new selection.
  const testNames = React.useMemo(
    () =>
      [selectedDevices.micDevice, selectedDevices.systemDevice].filter(
        (n): n is string => !!n
      ),
    [selectedDevices.micDevice, selectedDevices.systemDevice]
  );

  const isTestingRef = useRef(isTesting);
  isTestingRef.current = isTesting;
  useEffect(() => {
    if (!isTestingRef.current) {
      return;
    }

    voiceService
      .stopAudioLevelMonitoring()
      .then(() => voiceService.startAudioLevelMonitoring(testNames))
      .catch((error_) => setError(`Device test failed: ${String(error_)}`));
  }, [testNames]);

  useEffect(
    () => () => {
      void voiceService
        .stopAudioLevelMonitoring()
        .catch((error_) =>
          console.error("Device monitoring cleanup failed", error_)
        );
    },
    []
  );

  const toggleTest = async () => {
    try {
      if (isTesting) {
        await voiceService.stopAudioLevelMonitoring();
        setIsTesting(false);
        setLevels(null);
      } else {
        await voiceService.startAudioLevelMonitoring(testNames);
        setIsTesting(true);
      }
    } catch (error_) {
      console.error("audio level monitoring failed:", error_);
      setError("Failed to start the device test");
    }
  };

  if (loading) {
    return (
      <div className="animate-pulse p-4">
        <div className="mb-4 h-4 w-1/3 rounded bg-muted" />
        <div className="mb-3 h-10 rounded bg-muted" />
        <div className="h-10 rounded bg-muted" />
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h4 className="text-sm font-medium text-foreground">Audio Devices</h4>
        <div className="flex items-center space-x-2">
          <button
            type="button"
            onClick={() => void toggleTest()}
            disabled={disabled || !desktop}
            className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
              isTesting
                ? "bg-destructive/10 text-destructive hover:bg-destructive/10"
                : "bg-success/10 text-success hover:bg-success/10"
            } disabled:pointer-events-none disabled:opacity-50`}
          >
            {isTesting ? "Stop test" : "Test devices"}
          </button>
          <button
            type="button"
            onClick={() => {
              setRefreshing(true);
              void fetchDevices();
            }}
            disabled={refreshing || disabled}
            className="inline-flex h-8 w-8 items-center justify-center rounded-md p-0 hover:bg-muted/40 disabled:pointer-events-none disabled:opacity-50"
            title="Refresh devices"
          >
            <RefreshCw
              className={`h-4 w-4 ${refreshing ? "animate-spin" : ""}`}
            />
          </button>
        </div>
      </div>

      {error && (
        <div className="rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
          {error}
        </div>
      )}

      <DeviceSelect
        id="mic-selection"
        icon={<Mic className="h-4 w-4 text-muted-foreground" />}
        label="Microphone"
        defaultLabel="Default microphone"
        devices={inputDevices}
        value={selectedDevices.micDevice}
        onChange={(micDevice) =>
          onDeviceChange({ ...selectedDevices, micDevice })
        }
        disabled={disabled}
        level={isTesting ? (levels?.mic ?? 0) : null}
      />
      <DeviceSelect
        id="system-selection"
        icon={<Speaker className="h-4 w-4 text-muted-foreground" />}
        label="Call and system audio"
        defaultLabel="Default playback device"
        devices={outputDevices}
        value={selectedDevices.systemDevice}
        onChange={(systemDevice) =>
          onDeviceChange({ ...selectedDevices, systemDevice })
        }
        disabled={disabled}
        level={isTesting ? (levels?.system ?? 0) : null}
      />

      <div className="space-y-1.5 rounded-md border border-info/30 bg-info/10 p-3 text-xs text-foreground">
        <p>
          <strong>Microphone:</strong> your voice, normally the Jabra headset
          mic.
        </p>
        <p>
          <strong>Call and system audio:</strong> choose{" "}
          <strong>Google Chrome app audio (Google Voice)</strong> for Google
          Voice calls. It records only Chrome&apos;s sound, wherever it plays
          (the Jabra over Bluetooth included), so notification sounds and other
          apps stay out of the recording. Choose a playback device instead to
          record everything that device plays, e.g. a Teams or Zoom call in its
          own app.
        </p>
        <p>
          Use <strong>Test devices</strong> during a call: the system meter
          should move when the other side talks.
        </p>
      </div>
    </div>
  );
}

function DeviceSelect({
  id,
  icon,
  label,
  defaultLabel,
  devices,
  value,
  onChange,
  disabled,
  level,
}: {
  id: string;
  icon: React.ReactNode;
  label: string;
  defaultLabel: string;
  devices: AudioDevice[];
  value: string | null;
  onChange: (value: string | null) => void;
  disabled: boolean;
  level: number | null;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        {icon}
        <Label htmlFor={id}>{label}</Label>
      </div>
      <Select
        value={value || "default"}
        onValueChange={(v) => onChange(v === "default" ? null : v)}
        disabled={disabled}
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue placeholder={label} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="default">{defaultLabel}</SelectItem>
          {devices.map((device) => (
            <SelectItem key={deviceValue(device)} value={deviceValue(device)}>
              {deviceLabel(device)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {devices.length === 0 && (
        <p className="text-xs text-muted-foreground">No devices found</p>
      )}
      {level !== null && (
        <meter
          min={0}
          max={1}
          value={Math.min(1, Math.max(0, level))}
          aria-label={`${label} level`}
          className="h-2 w-full"
        />
      )}
    </div>
  );
}
