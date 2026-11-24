import { describe, expect, test } from "bun:test";

import type { CalendarEvent } from "@/types/voice";

import {
  callContacts,
  orderedCalls,
  eventsToday,
  suggestedMode,
} from "./recording-context";

const event: CalendarEvent = {
  id: "x",
  title: "Weekly engineering",
  start: "2026-11-16T16:30:00Z",
  end: "2026-11-16T17:30:00Z",
  location: "2F Large Conference Room",
  online: true,
  organizer: null,
  invitees: [],
};

describe("calendar recording context", () => {
  test("an automatically added Teams link does not override a physical room", () => {
    expect(suggestedMode(event)).toBe("in_person");
    expect(
      suggestedMode({ ...event, location: "Microsoft Teams Meeting" })
    ).toBe("online");
    expect(suggestedMode({ ...event, location: "", online: false })).toBe(
      "in_person"
    );
  });
  test("today follows Arizona midnight rather than the computer timezone", () => {
    expect(eventsToday([event], new Date("2026-11-17T06:59:00Z"))).toHaveLength(
      1
    );
    expect(eventsToday([event], new Date("2026-11-17T07:00:00Z"))).toHaveLength(
      0
    );
  });
});

test("call priorities respect the Phoenix day and keep undated calls last", () => {
  const base = { id: "", title: "", number: "", company: "", source: "" };

  const calls = [
    { ...base, title: "undated", due: "", highPriority: true },
    { ...base, title: "future", due: "2026-11-25T16:00Z", highPriority: true },
    { ...base, title: "normal", due: "2026-11-24T14:00Z" },
    { ...base, title: "urgent", due: "2026-11-24T14:30Z", highPriority: true },
  ];

  expect(
    orderedCalls(calls, Date.parse("2026-11-25T01:00Z")).map((c) => c.title)
  ).toEqual(["urgent", "normal", "future", "undated"]);
});

test("an ordered call keeps alternate people and numbers on the same task", () => {
  const task = {
    id: "t",
    due: "",
    company: "Supplier",
    source: "",
    number: "+12025550100",
    title:
      "Call Frank Smith at 202-555-0100 to confirm a tour. If Frank is unavailable, call Jake Zemer at 202-555-0101, then the office at 202-555-0102.",
  };

  expect(callContacts(task)).toEqual([
    { name: "Frank Smith", number: "+12025550100" },
    { name: "Jake Zemer", number: "+12025550101" },
    { name: "office", number: "+12025550102" },
  ]);
});
