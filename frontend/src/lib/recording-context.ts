import type { CallTask, CalendarEvent, RecordingMode } from "@/types/voice";

export const RECORDING_MODES: {
  value: RecordingMode;
  label: string;
  description: string;
}[] = [
  {
    value: "in_person",
    label: "In person",
    description: "Microphone only. Separates the people speaking in the room.",
  },
  {
    value: "online",
    label: "Online meeting",
    description:
      "Microphone and computer audio. Separates speakers on both sides, including shared rooms.",
  },
  {
    value: "phone",
    label: "Phone call",
    description:
      "Microphone and computer call audio. For a phone on speaker beside the mic, choose In person.",
  },
];

export function suggestedMode(event: CalendarEvent): RecordingMode {
  // A physical room takes precedence over Teams links automatically added to invitations.
  const location = event.location.trim();

  const virtualLocation =
    /^(microsoft teams( meeting)?|teams|zoom( meeting)?|google meet|online|https?:\/\/.*)$/i.test(
      location
    );

  return location && !virtualLocation
    ? "in_person"
    : event.online
      ? "online"
      : "in_person";
}

export function eventsToday(events: CalendarEvent[], now = new Date()) {
  const day = (date: Date) =>
    new Intl.DateTimeFormat("en-CA", {
      timeZone: "America/Phoenix",
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(date);

  return events
    .filter((event) => day(new Date(event.start)) === day(now))
    .toSorted((a, b) => Date.parse(a.start) - Date.parse(b.start));
}

const callDate = new Intl.DateTimeFormat("en-US", {
  timeZone: "America/Phoenix",
  weekday: "short",
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
  timeZoneName: "short",
});

export function callSchedule(call: CallTask) {
  const due = Date.parse(call.due);

  return [
    call.highPriority ? "High priority" : "Normal priority",
    Number.isFinite(due) ? callDate.format(due) : "Date needed",
  ].join(" · ");
}

export function orderedCalls(calls: CallTask[], now: number) {
  const day = new Intl.DateTimeFormat("en-CA", {
    timeZone: "America/Phoenix",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });

  const today = day.format(now);

  function rank(call: CallTask) {
    const time = Date.parse(call.due);

    if (!Number.isFinite(time)) {
      return ["2", "", 0, 0] as const;
    }

    const date = day.format(time);

    return [
      date <= today ? "0" : "1",
      date <= today ? "" : date,
      call.highPriority ? 0 : 1,
      time,
    ] as const;
  }

  return calls.toSorted((a, b) => {
    const x = rank(a);
    const y = rank(b);

    return (
      x[0].localeCompare(y[0]) ||
      x[1].localeCompare(y[1]) ||
      x[2] - y[2] ||
      x[3] - y[3] ||
      a.title.localeCompare(b.title)
    );
  });
}
