"use client";

import { useSearchParams } from "next/navigation";
import { useState, Suspense, Fragment } from "react";
import { toast } from "sonner";

import { AgendaRow, agendaDay, agendaTime } from "@/components/AgendaRow";
import { CallAttempts } from "@/components/CallAttempts";
import { CallTaskActions } from "@/components/CallTaskActions";
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
  calendarPhase,
  calendarTimeLeft,
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

  const [tab, setTab] = useState(
    initialTask.startsWith("event:") ? "meetings" : "calls"
  );

  const [selected, setSelected] = useState(initialTask);
  const { enabled, setEnabled } = usePublicationDefault();
  const { isRecording, isStartingRecording, isStopping } = useRecordingState();

  const { startRecording } = useRecordingStart(() =>
    toast.error("Activate recording in Settings.")
  );

  const calls = orderedCalls(snapshot?.calls ?? [], now).map((c) => ({
    key: `task:${c.id}`,
    editable: c.editable,
    title: c.title,
    detail: callPurpose(c),
    company: c.company.split(" | ")[0],
    day:
      c.due && Number.isFinite(Date.parse(c.due))
        ? agendaDay(c.due)
        : "Date needed",
    time: c.due && Number.isFinite(Date.parse(c.due)) ? agendaTime(c.due) : "",
    contacts: callContacts(c),
    high: c.highPriority,
    overdue: Boolean(c.due && Date.parse(c.due) < now),
    number: c.number,
    event: null,
  }));

  const meetings = (snapshot?.events ?? [])
    .filter((e) => Date.parse(e.end) >= now)
    .toSorted((a, b) => Date.parse(a.start) - Date.parse(b.start))
    .map((e) => ({
      key: `event:${e.id}`,
      editable: false,
      title: e.title,
      detail: e.location,
      day: agendaDay(e.start),
      time: agendaTime(e.start),
      overdue: false,
      contacts: [],
      company: "",
      high: false,
      number: "",
      event: e,
    }));

  const item = [...calls, ...meetings].find((x) => x.key === selected);

  async function start(contact?: CallContact) {
    if (
      !item ||
      enabled === null ||
      (item.event && calendarPhase(item.event, now) === "ended")
    ) {
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
        <TabsContent value={tab} className="mt-4">
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
                      <h2 className="font-semibold not-first:pt-3">
                        {row.day} {row.time && <small>Phoenix</small>}
                      </h2>
                    )}
                    <AgendaRow
                      time={row.time}
                      title={row.company || row.title}
                      detail={
                        row.event
                          ? row.detail
                          : [row.contacts[0]?.name, row.detail]
                              .filter(Boolean)
                              .join(" · ")
                      }
                      status={
                        row.event && calendarPhase(row.event, now) === "current"
                          ? calendarTimeLeft(row.event.end, now)
                          : [
                              row.overdue && "Past due",
                              row.high && "High priority",
                            ]
                              .filter(Boolean)
                              .join(" · ")
                      }
                      selected={row.key === selected}
                      onSelect={() => setSelected(row.key)}
                    />
                  </Fragment>
                ))}
                {tab === "calls" && snapshot?.calls_stale && (
                  <p role="status" className="text-sm text-muted-foreground">
                    Task sync is delayed. Showing the last available call list.
                  </p>
                )}
                {checked &&
                  !(tab === "calls" && snapshot?.calls_stale) &&
                  !(tab === "calls" ? calls : meetings).length && (
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
                  {tab === "calls" ? "Back to calls" : "Back to meetings"}
                </Button>
                <h2 className="font-semibold">{item.company || item.title}</h2>
                {item.detail !== (item.company || item.title) && (
                  <p className="text-sm">{item.detail}</p>
                )}
                {!item.event && (
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
                {item.editable && (
                  <CallTaskActions key={item.key} id={item.key.slice(5)} />
                )}
                {item.event ? (
                  <MeetingBrief key={item.key} itemKey={item.key} />
                ) : (
                  <details key={`notes:${item.key}`}>
                    <summary>Call notes</summary>
                    <MeetingBrief itemKey={item.key} showAttempts={false} />
                  </details>
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
