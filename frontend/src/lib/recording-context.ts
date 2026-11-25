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

export function calendarPhase(event: CalendarEvent, now: number) {
  if (now >= Date.parse(event.end)) {
    return "ended";
  }

  return now >= Date.parse(event.start) ? "current" : "upcoming";
}

export function calendarTimeLeft(end: string, now: number) {
  const minutes = Math.ceil((Date.parse(end) - now) / 60_000);

  return minutes > 0 ? `${minutes} min left` : "Scheduled time ended";
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
      date,
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

/** The assigned CRM number is authoritative. Additional destinations must be
 * explicitly written as “call Name at number” or “then Name at number”. */
export function callContacts(task: CallTask) {
  const primary = task.title.match(
    /^call (?:or email )?(.+?)(?= again\b| at\b| about\b| if\b| to\b| for\b| re\b|[,;:]|$)/i
  )?.[1];

  const contacts = task.number
    ? [
        {
          name: primary || task.company || "Primary contact",
          number: canonicalNumber(task.number) || task.number,
        },
      ]
    : [];

  for (const match of task.title.matchAll(
    /[,;]\s*(?:(?:then\s+)?call\s+|then\s+)([\p{L}][\p{L}'’ -]{0,50}?)\s+at\s+(\+?\(?\d[\d ()-]{5,20}\d)(?=[.,;\s]|$)/giu
  )) {
    const number = canonicalNumber(match[2]);

    if (number && !contacts.some((c) => c.number === number)) {
      contacts.push({ name: match[1].replace(/^the /i, ""), number });
    }
  }

  return contacts;
}

function canonicalNumber(raw: string) {
  const digits = raw.replaceAll(/\D/g, "");

  if (raw.startsWith("+") && digits.length >= 7 && digits.length <= 15) {
    return `+${digits}`;
  }

  if (digits.length === 10) {
    return `+1${digits}`;
  }

  return digits.length === 11 && digits.startsWith("1") ? `+${digits}` : "";
}

export function callPurpose(task: CallTask) {
  const [first] = task.title.split(/\.\s+If [^.,]+,\s*call /i);

  const match = first.match(
    /^call (?:or email )?[^,;:.]+?(?: at [+(\d][+\d() -]+)? (to|about|if|again if) (.+)/i
  );

  if (!match) {
    return task.title;
  }

  const reason = (
    match[1].endsWith("if") ? `Follow up if ${match[2]}` : match[2]
  ).replace(/:\s*[+\d][\d ()-]{6,}$/, "");

  return reason.charAt(0).toUpperCase() + reason.slice(1);
}
