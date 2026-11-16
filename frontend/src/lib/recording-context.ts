import type { CalendarEvent, RecordingMode } from "@/types/voice";

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
