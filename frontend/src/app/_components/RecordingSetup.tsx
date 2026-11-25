import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { AgendaRow, agendaTime } from "@/components/AgendaRow";
import { Button } from "@/components/ui/button";
import { useCalendar } from "@/contexts/CalendarContext";
import { useMeetings } from "@/contexts/MeetingContext";
import {
  calendarPhase,
  calendarTimeLeft,
  eventsToday,
  RECORDING_MODES,
} from "@/lib/recording-context";
import type { CalendarEvent, RecordingMode } from "@/types/voice";

export function RecordingType({
  mode,
  onModeChange,
}: {
  mode: RecordingMode;
  onModeChange: (mode: RecordingMode) => void;
}) {
  return (
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
  );
}

export function RecordingSetup({
  canStart,
  onEventChange,
}: {
  canStart: boolean;
  onEventChange: (event: CalendarEvent) => void;
}) {
  const { snapshot, error, now, checked } = useCalendar();
  const router = useRouter();
  const { meetings } = useMeetings();
  const [selectedDay, setSelectedDay] = useState<number | null>(null);
  const days = 86_400_000;
  const displayDay = selectedDay ?? now;
  const events = eventsToday(snapshot?.events ?? [], new Date(displayDay));

  const formatDay = (value: number) =>
    new Date(value).toLocaleDateString("en-US", {
      timeZone: "America/Phoenix",
      weekday: "short",
      month: "short",
      day: "numeric",
    });

  const selectedToday = formatDay(displayDay) === formatDay(now);
  const scheduled = snapshot?.events ?? [];

  return (
    <div className="space-y-4">
      <section className="space-y-2">
        <div className="flex items-center justify-between">
          <h2 className="flex items-center gap-2 text-sm font-medium">
            <CalendarDays className="size-4" />
            {selectedToday ? "Today’s meetings" : formatDay(displayDay)}
          </h2>
          <div className="flex items-center">
            {!selectedToday && (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => setSelectedDay(null)}
              >
                Today
              </Button>
            )}
            <Button
              type="button"
              size="icon"
              variant="ghost"
              aria-label="Previous day"
              disabled={
                !scheduled.some(
                  (e) =>
                    Date.parse(e.start) < displayDay &&
                    formatDay(Date.parse(e.start)) !== formatDay(displayDay)
                )
              }
              onClick={() => setSelectedDay(displayDay - days)}
            >
              <ChevronLeft />
            </Button>
            <Button
              type="button"
              size="icon"
              variant="ghost"
              aria-label="Next day"
              disabled={
                !scheduled.some(
                  (e) =>
                    Date.parse(e.start) > displayDay &&
                    formatDay(Date.parse(e.start)) !== formatDay(displayDay)
                )
              }
              onClick={() => setSelectedDay(displayDay + days)}
            >
              <ChevronRight />
            </Button>
          </div>
        </div>
        {events.map((item) => {
          const phase = calendarPhase(item, now);

          const recording = meetings.find(
            (m) => m.brief_key === `event:${item.id}`
          );

          const ended = phase === "ended";

          return (
            <AgendaRow
              key={item.id}
              title={item.title}
              time={agendaTime(item.start)}
              muted={ended}
              detail={item.location}
              status={
                phase === "current"
                  ? calendarTimeLeft(item.end, now)
                  : ended
                    ? recording
                      ? "Ended"
                      : "No linked recording"
                    : undefined
              }
              actions={
                <>
                  {recording && (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        router.push(
                          `/meeting-details?id=${encodeURIComponent(recording.id)}`
                        )
                      }
                    >
                      View recording
                    </Button>
                  )}
                  {!ended && (
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      disabled={!canStart}
                      onClick={() => {
                        if (calendarPhase(item, Date.now()) !== "ended") {
                          onEventChange(item);
                        }
                      }}
                    >
                      Start
                    </Button>
                  )}
                </>
              }
            />
          );
        })}
        {error && (
          <p role="status" className="text-xs text-muted-foreground">
            {snapshot ? "Showing saved calendar. " : ""}
            {error}
          </p>
        )}
        {checked && !error && !events.length && (
          <p className="text-xs text-muted-foreground">
            {snapshot
              ? "No meetings scheduled for this day."
              : "Connect Outlook in Settings to see scheduled meetings."}
          </p>
        )}
      </section>
    </div>
  );
}
