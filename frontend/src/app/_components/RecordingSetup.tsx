import { CalendarDays } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useCalendar } from "@/contexts/CalendarContext";
import { eventsToday, RECORDING_MODES } from "@/lib/recording-context";
import type { CalendarEvent, RecordingMode } from "@/types/voice";

export function RecordingSetup({
  mode,
  onModeChange,
  canStart,
  onEventChange,
}: {
  mode: RecordingMode;
  onModeChange: (mode: RecordingMode) => void;
  canStart: boolean;
  onEventChange: (event: CalendarEvent) => void;
}) {
  const { snapshot, error, now, checked } = useCalendar();
  const events = eventsToday(snapshot?.events ?? [], new Date(now));

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
      <section className="space-y-2">
        <h2 className="flex items-center gap-2 text-sm font-medium">
          <CalendarDays className="size-4" />
          Today’s meetings
        </h2>
        {events.map((item) => (
          <Button
            className="h-auto w-full justify-between text-left whitespace-normal"
            type="button"
            variant="outline"
            disabled={!canStart}
            key={item.id}
            onClick={() => onEventChange(item)}
          >
            <span>
              {new Date(item.start).toLocaleTimeString("en-US", {
                hour: "numeric",
                minute: "2-digit",
                timeZone: "America/Phoenix",
              })}{" "}
              · {item.title}
            </span>
            <span>Start</span>
          </Button>
        ))}
        {error && (
          <p role="status" className="text-xs text-muted-foreground">
            {snapshot ? "Showing saved calendar. " : ""}
            {error}
          </p>
        )}
        {checked && !error && !events.length && (
          <p className="text-xs text-muted-foreground">
            {snapshot
              ? "No meetings scheduled today."
              : "Connect Outlook in Settings to see scheduled meetings."}
          </p>
        )}
      </section>
    </div>
  );
}
