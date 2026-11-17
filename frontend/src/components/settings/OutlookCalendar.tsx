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
  const [checking, setChecking] = useState(isTauri);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  useEffect(() => {
    let current = true;

    if (isTauri()) {
      void invoke<Connection>("outlook_status")
        .then((result) => {
          if (current) {
            setConnection(result);
          }
        })
        .catch((error) => {
          if (current) {
            setMessage(errorMessage(error));
          }
        })
        .finally(() => {
          if (current) {
            setChecking(false);
          }
        });
    }

    return () => {
      current = false;
    };
  }, []);

  const connect = async () => {
    setBusy(true);
    setMessage(
      "Sign in to your work Outlook account in the Edge window. Keep this app open."
    );

    try {
      const next = await invoke<Connection>("outlook_connect");
      setConnection(next);
      setMessage("");
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
      setMessage("");
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

  const connected = Boolean(connection?.account && !connection.error);

  return (
    <SettingsSection
      title="Outlook calendar"
      description="Sign in to your own Outlook Web account. Meetily reads your calendar to help start meetings; calendar changes stay in Outlook."
    >
      <p role="status" className="text-sm">
        {checking && "Checking Outlook…"}
        {!checking && (connected ? "Connected" : "Disconnected")}
        {!checking && connection?.account && ` · ${connection.account}`}
      </p>
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
      {!checking &&
        (connected ? (
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => void disconnect()}
          >
            Disconnect
          </Button>
        ) : (
          <Button disabled={busy} onClick={() => void connect()}>
            Connect Outlook
          </Button>
        ))}
    </SettingsSection>
  );
}
