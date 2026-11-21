"use client";

import { createContext, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";

import { useDesktopRuntime } from "@/hooks/useDesktopRuntime";
import { voiceService, errorMessage } from "@/services/voiceService";
import type { CalendarSnapshot } from "@/types/voice";

const key = "meetily-calendar-v1";
const changed = "meetily-calendar-changed";

export function calendarChanged() {
  localStorage.removeItem(key);
  window.dispatchEvent(new Event(changed));
}

const CalendarContext = createContext<{
  snapshot: CalendarSnapshot | null;
  error: string;
  now: number;
  checked: boolean;
}>({ snapshot: null, error: "", now: 0, checked: false });

export const useCalendar = () => useContext(CalendarContext);

export function CalendarProvider({ children }: { children: ReactNode }) {
  const desktop = useDesktopRuntime();
  const [snapshot, setSnapshot] = useState<CalendarSnapshot | null>(null);
  const [error, setError] = useState("");
  const [now, setNow] = useState(0);
  const [checked, setChecked] = useState(false);
  useEffect(() => {
    if (!desktop) {
      return;
    }

    let alive = true;
    let revision = 0;
    let pending = false;
    let again = false;

    try {
      const cached: CalendarSnapshot | null = JSON.parse(
        localStorage.getItem(key) ?? "null"
      );

      if (
        cached &&
        Array.isArray(cached.events) &&
        Date.now() - Date.parse(cached.observed) < 86_400_000
      ) {
        setSnapshot(cached);
      }
    } catch {
      localStorage.removeItem(key);
    }

    const refresh = async () => {
      setNow(Date.now());

      if (pending) {
        return;
      }

      pending = true;
      const version = revision;

      try {
        const next = await voiceService.getRecordingCalendar();

        if (alive && revision === version) {
          setSnapshot(next);
          setError("");

          if (next) {
            localStorage.setItem(key, JSON.stringify(next));
          } else {
            localStorage.removeItem(key);
          }
        }
      } catch (error_) {
        if (alive && revision === version) {
          setError(errorMessage(error_));
        }
      } finally {
        pending = false;

        if (alive && revision === version) {
          setChecked(true);
        }

        if (alive && again) {
          again = false;
          void refresh();
        }
      }
    };

    const invalidate = () => {
      revision++;
      setSnapshot(null);
      setChecked(false);
      setError("");
      again = pending;
      void refresh();
    };

    void refresh();
    const timer = window.setInterval(refresh, 60_000);
    window.addEventListener(changed, invalidate);
    window.addEventListener("focus", refresh);

    return () => {
      alive = false;
      window.clearInterval(timer);
      window.removeEventListener(changed, invalidate);
      window.removeEventListener("focus", refresh);
    };
  }, [desktop]);

  return (
    <CalendarContext.Provider value={{ snapshot, error, now, checked }}>
      {children}
    </CalendarContext.Provider>
  );
}
