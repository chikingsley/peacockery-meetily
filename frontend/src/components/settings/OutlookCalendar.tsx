"use client";

import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from "@/components/ui/select";
import { errorMessage } from "@/services/voiceService";

import { SettingsSection } from "./SettingsParts";

interface Connection {
  account: string | null;
  calendars: { id: string; name: string; owner: string }[];
  selected: string;
  error: string | null;
}

export function OutlookCalendar() {
  const [connection, setConnection] = useState<Connection | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  useEffect(() => {
    if (isTauri()) {
      void invoke<Connection>("outlook_status")
        .then(setConnection)
        .catch((error) => setMessage(errorMessage(error)));
    }
  }, []);

  const connect = async () => {
    setBusy(true);
    setMessage(
      "Sign in to your work Outlook account in the Edge window. Keep this app open."
    );

    try {
      const next = await invoke<Connection>("outlook_connect");
      setConnection(next);
      setMessage(`Connected to ${next.account}.`);
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);

    try {
      await invoke("outlook_disconnect");
      setConnection(null);
      setMessage("Calendar disconnected. Your meetings stay saved.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const select = async (calendar: string) => {
    setBusy(true);

    try {
      await invoke("outlook_select", { calendar });
      setConnection((current) =>
        current ? { ...current, selected: calendar } : current
      );
      setMessage("Calendar selected.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingsSection
      title="Outlook calendar"
      description="Sign in to your own Outlook Web account. Meetily reads your calendar to help start meetings; calendar changes stay in Outlook."
    >
      {connection?.account && (
        <p className="text-sm">Connected account: {connection.account}</p>
      )}
      {connection?.calendars.length ? (
        <Select
          value={connection.selected}
          onValueChange={(calendar) => void select(calendar)}
          disabled={busy}
        >
          <SelectTrigger aria-label="Outlook calendar">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {connection.calendars.map((calendar) => (
              <SelectItem key={calendar.id} value={calendar.id}>
                {calendar.name}
                {calendar.owner ? ` · ${calendar.owner}` : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : null}
      {(message || connection?.error) && (
        <p role="status" className="text-sm text-muted-foreground">
          {message || connection?.error}
        </p>
      )}
      <div className="flex gap-2">
        <Button disabled={busy} onClick={() => void connect()}>
          {connection?.account ? "Reconnect Outlook" : "Connect Outlook"}
        </Button>
        {connection?.account && (
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => void disconnect()}
          >
            Disconnect
          </Button>
        )}
      </div>
      <p className="text-xs text-muted-foreground">
        A separate Edge profile keeps this connection on this PC. Reconnect when
        Outlook requests a fresh sign-in.
      </p>
    </SettingsSection>
  );
}
