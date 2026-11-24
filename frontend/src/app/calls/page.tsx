"use client";

import { useSearchParams } from "next/navigation";
import { useState, Suspense, Fragment } from "react";
import { toast } from "sonner";

import { CallAttempts } from "@/components/CallAttempts";
import { MeetingBrief } from "@/components/MeetingBrief";
import {
  PublicationChoice,
  usePublicationDefault,
} from "@/components/Publication";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useCalendar } from "@/contexts/CalendarContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { useRecordingStart } from "@/hooks/useRecordingStart";
import {
  callContacts,
  callPurpose,
  orderedCalls,
  suggestedMode,
} from "@/lib/recording-context";
import { errorMessage, voiceService } from "@/services/voiceService";
import type { CallContact } from "@/types/voice";

export default function Calls() {
  return (
    <Suspense>
      <CallsRoute />
    </Suspense>
  );
}

function CallsRoute() {
  const params = useSearchParams();

  return (
    <CallsContent
      key={params.toString()}
      initialTask={params.get("task") ?? ""}
    />
  );
}

function CallsContent({ initialTask }: { initialTask: string }) {
  const { snapshot, now, checked, error } = useCalendar();
  const [tab, setTab] = useState("calls");
  const [selected, setSelected] = useState(initialTask);
  const { enabled, setEnabled } = usePublicationDefault();
  const { isRecording, isStartingRecording, isStopping } = useRecordingState();

  const { startRecording } = useRecordingStart(() =>
    toast.error("Activate recording in Settings.")
  );

  const calls = orderedCalls(snapshot?.calls ?? [], now).map((c) => ({
    key: `task:${c.id}`,
    title: c.title,
    detail: callPurpose(c),
    company: c.company.split(" | ")[0],
    day:
      c.due && Number.isFinite(Date.parse(c.due))
        ? new Date(c.due).toLocaleDateString("en-US", {
            timeZone: "America/Phoenix",
            weekday: "long",
            month: "long",
            day: "numeric",
          })
        : "Date needed",
    time:
      c.due && Number.isFinite(Date.parse(c.due))
        ? new Date(c.due).toLocaleTimeString("en-US", {
            timeZone: "America/Phoenix",
            hour: "numeric",
            minute: "2-digit",
          })
        : "",
    contacts: callContacts(c),
    high: c.highPriority,
    number: c.number,
    event: null,
  }));

  const meetings = (snapshot?.events ?? [])
    .filter((e) => Date.parse(e.end) >= now)
    .map((e) => ({
      key: `event:${e.id}`,
      title: e.title,
      detail: new Date(e.start).toLocaleString(undefined, {
        weekday: "short",
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
      }),
      day: "",
      time: "",
      contacts: [],
      company: "",
      high: false,
      number: "",
      event: e,
    }));

  const item = [...calls, ...meetings].find((x) => x.key === selected);

  async function start(contact?: CallContact) {
    if (!item || enabled === null) {
      return;
    }

    const id = await startRecording(
      item.title,
      item.event ? suggestedMode(item.event) : "phone",
      item.event,
      enabled,
      item.key,
      contact
    );

    if (id) {
      window.history.pushState(null, "", "/");

      if (contact?.number) {
        try {
          await voiceService.dialPhone(contact.number);
        } catch (error_) {
          toast.error("Recording started. Open Google Voice in Chrome.", {
            description: errorMessage(error_),
          });
        }
      }
    }
  }

  return (
    <main className="mx-auto h-full w-full max-w-4xl overflow-y-auto px-5 py-6">
      <Tabs
        value={tab}
        onValueChange={(next) => {
          setTab(next);
          setSelected("");
        }}
      >
        <TabsList aria-label="Workspace">
          <TabsTrigger value="calls">Call Tasks</TabsTrigger>
          <TabsTrigger value="meetings">Meeting Prep</TabsTrigger>
        </TabsList>
        <TabsContent value={tab}>
          <div
            className={
              item
                ? "grid items-start gap-6 md:grid-cols-2"
                : "grid items-start"
            }
          >
            <div className={item ? "hidden space-y-5 md:block" : "space-y-5"}>
              <section className="space-y-2">
                {tab === "meetings" && (
                  <h2 className="text-sm font-semibold">Upcoming Meetings</h2>
                )}
                {(tab === "calls" ? calls : meetings).map((row, i, rows) => (
                  <Fragment key={row.key}>
                    {row.day && row.day !== rows[i - 1]?.day && (
                      <h2 className="pt-3 font-semibold">
                        {row.day} {row.time && <small>Phoenix</small>}
                      </h2>
                    )}
                    <Button
                      type="button"
                      variant={row.key === selected ? "secondary" : "outline"}
                      className="h-auto w-full flex-col items-start text-left whitespace-normal"
                      onClick={() => setSelected(row.key)}
                    >
                      <span className="flex flex-wrap gap-3">
                        <strong>{row.time}</strong>
                        {row.company || row.title}{" "}
                        {row.high && <small>High priority</small>}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {row.day
                          ? [row.contacts[0]?.name, row.detail]
                              .filter(Boolean)
                              .join(" · ")
                          : row.detail}
                      </span>
                    </Button>
                  </Fragment>
                ))}
                {checked && !(tab === "calls" ? calls : meetings).length && (
                  <p className="text-sm text-muted-foreground">
                    {tab === "calls"
                      ? "No assigned calls in this agenda."
                      : "No upcoming meetings in this agenda."}
                  </p>
                )}
              </section>
              {error && (
                <p role="status" className="text-sm">
                  {error}
                </p>
              )}
            </div>
            {item && (
              <div className="space-y-4 md:sticky md:top-0">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="md:hidden"
                  onClick={() => setSelected("")}
                >
                  Back to calls
                </Button>
                <h2 className="font-semibold">{item.company || item.title}</h2>
                {item.detail !== (item.company || item.title) && (
                  <p className="text-sm">{item.detail}</p>
                )}
                {item.day && (
                  <CallAttempts
                    key={item.key}
                    taskKey={item.key}
                    contacts={item.contacts}
                    disabled={
                      enabled === null ||
                      isRecording ||
                      isStartingRecording ||
                      isStopping
                    }
                    onCall={(c) => void start(c)}
                  />
                )}
                {item.day ? (
                  <details key={`notes:${item.key}`}>
                    <summary>Call notes</summary>
                    <MeetingBrief itemKey={item.key} showAttempts={false} />
                  </details>
                ) : (
                  <MeetingBrief key={item.key} itemKey={item.key} />
                )}
                <PublicationChoice enabled={enabled} onChange={setEnabled} />
                {!item.contacts.length && (
                  <Button
                    disabled={
                      enabled === null ||
                      isRecording ||
                      isStartingRecording ||
                      isStopping
                    }
                    onClick={() => void start()}
                  >
                    Start recording
                  </Button>
                )}
              </div>
            )}
          </div>
        </TabsContent>
      </Tabs>
    </main>
  );
}
