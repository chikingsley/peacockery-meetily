import { CalendarDays } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useDesktopRuntime } from "@/hooks/useDesktopRuntime";
import { eventsToday, RECORDING_MODES } from "@/lib/recording-context";
import { voiceService, errorMessage } from "@/services/voiceService";
import type {
  CalendarEvent,
  CalendarSnapshot,
  RecordingMode,
} from "@/types/voice";

export function RecordingSetup({
  mode,
  onModeChange,
  event,
  onEventChange,
}: {
  mode: RecordingMode;
  onModeChange: (mode: RecordingMode) => void;
  event: CalendarEvent | null;
  onEventChange: (event: CalendarEvent | null) => void;
}) {
  const desktop = useDesktopRuntime();
  const [snapshot, setSnapshot] = useState<CalendarSnapshot | null>(null);
  const [checkedAt, setCheckedAt] = useState(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    if (!desktop) {
      return;
    }

    let active = true;
    let reading = false;

    const read = async () => {
      if (reading) {
        return;
      }

      reading = true;

      try {
        const next = await voiceService.getRecordingCalendar();

        if (active) {
          setCheckedAt(Date.now());
          setSnapshot(next);
          setError(null);
        }
      } catch (error_) {
        if (active) {
          setError(errorMessage(error_));
        }
      } finally {
        reading = false;

        if (active) {
          setLoading(false);
        }
      }
    };

    void read();

    const timer = setInterval(() => {
      void read();
    }, 30_000);

    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [desktop]);
  const events = eventsToday(snapshot?.events ?? [], new Date(checkedAt));

  return (
    <div className="space-y-4">
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">Recording type</legend>
        <div className="flex flex-wrap gap-2">
          {RECORDING_MODES.map((option) => (
            <Button
              key={option.value}
              type="button"
              variant={mode === option.value ? "default" : "outline"}
              aria-pressed={mode === option.value}
              onClick={() => onModeChange(option.value)}
            >
              {option.label}
            </Button>
          ))}
        </div>
        <p className="text-sm text-muted-foreground">
          {RECORDING_MODES.find((option) => option.value === mode)?.description}
        </p>
      </fieldset>
      <div className="space-y-2">
        <Label htmlFor="calendar-meeting">
          <CalendarDays className="size-4" />
          Today’s calendar
        </Label>
        <Select
          value={event?.id ?? "none"}
          onValueChange={(id) =>
            onEventChange(events.find((item) => item.id === id) ?? null)
          }
        >
          <SelectTrigger id="calendar-meeting" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">Unscheduled meeting</SelectItem>
            {events.map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {new Date(item.start).toLocaleTimeString("en-US", {
                  hour: "numeric",
                  minute: "2-digit",
                  timeZone: "America/Phoenix",
                })}{" "}
                – {item.title}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {loading && desktop
            ? "Loading calendar… Recording is available."
            : error
              ? `Calendar refresh needs attention: ${error}`
              : snapshot
                ? "Calendar times are Arizona time. The invitation is saved with the recording."
                : "Connect Outlook in Settings to choose calendar meetings. You can record an unscheduled meeting."}
        </p>
        {event && (
          <p className="text-xs text-muted-foreground">
            {event.location || "Location not listed"} · {event.invitees.length}{" "}
            invited. Invitations provide context; speaker names need
            verification.
          </p>
        )}
      </div>
    </div>
  );
}
