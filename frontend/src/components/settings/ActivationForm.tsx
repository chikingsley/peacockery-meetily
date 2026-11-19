"use client";

import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useConfig } from "@/contexts/ConfigContext";
import { errorMessage } from "@/services/voiceService";

export function ActivationForm({ onActivated }: { onActivated?: () => void }) {
  const { reloadVoiceSettings, refreshKeyStatus, refreshVoiceModels } =
    useConfig();

  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const activate = async () => {
    setBusy(true);
    setError("");

    try {
      const recipient = await invoke<string>("activate_access", { code });

      await reloadVoiceSettings();
      await refreshKeyStatus();
      await refreshVoiceModels();
      setCode("");
      toast.success(`Access activated for ${recipient}`);
      onActivated?.();
    } catch (error_) {
      setError(errorMessage(error_));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="space-y-2"
      onSubmit={(event) => {
        event.preventDefault();
        void activate();
      }}
    >
      <div className="flex gap-2">
        <Input
          aria-label="Activation code"
          inputMode="numeric"
          autoComplete="one-time-code"
          placeholder="1234 5678"
          maxLength={9}
          disabled={busy}
          value={code}
          onChange={(event) =>
            setCode(event.target.value.replaceAll(/[^\d ]/g, ""))
          }
        />
        <Button
          type="submit"
          disabled={busy || code.replaceAll(/\s/g, "").length !== 8}
        >
          {busy ? "Activating…" : "Activate"}
        </Button>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
    </form>
  );
}
