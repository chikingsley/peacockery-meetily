"use client";

import { useState } from "react";
import { toast } from "sonner";

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
import { suggestedMode } from "@/lib/recording-context";
import { errorMessage, voiceService } from "@/services/voiceService";

export default function Calls() {
  const { snapshot, now, checked, error } = useCalendar();
  const [tab, setTab] = useState("calls");
  const [selected, setSelected] = useState("");
  const { enabled, setEnabled } = usePublicationDefault();
  const { isRecording, isStartingRecording, isStopping } = useRecordingState();

  const { startRecording } = useRecordingStart(() =>
    toast.error("Activate recording in Settings.")
  );

  const calls = (snapshot?.calls ?? []).map((c) => ({
    key: `task:${c.id}`,
    title: c.title,
    detail: [c.company, c.number].filter(Boolean).join(" · "),
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
      number: "",
      event: e,
    }));

  const item = [...calls, ...meetings].find((x) => x.key === selected);

  async function start() {
    if (!item || enabled === null) {
      return;
    }

    const id = await startRecording(
      item.title,
      item.event ? suggestedMode(item.event) : "phone",
      item.event,
      enabled,
      item.key
    );

    if (id) {
      window.history.pushState(null, "", "/");

      if (item.number) {
        try {
          await voiceService.dialPhone(item.number);
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
          <div className="grid items-start gap-6 md:grid-cols-2">
            <div className={item ? "hidden space-y-5 md:block" : "space-y-5"}>
              <section className="space-y-2">
                {tab === "meetings" && (
                  <h2 className="text-sm font-semibold">Upcoming Meetings</h2>
                )}
                {(tab === "calls" ? calls : meetings).map((row) => (
                  <Button
                    key={row.key}
                    type="button"
                    variant={row.key === selected ? "secondary" : "outline"}
                    className="h-auto w-full flex-col items-start text-left whitespace-normal"
                    onClick={() => setSelected(row.key)}
                  >
                    <span>{row.title}</span>
                    <span className="text-xs text-muted-foreground">
                      {row.detail}
                    </span>
                  </Button>
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
                <h2 className="font-semibold">{item.title}</h2>
                <p className="text-sm">{item.detail}</p>
                <MeetingBrief key={item.key} itemKey={item.key} />
                <PublicationChoice enabled={enabled} onChange={setEnabled} />
                <Button
                  disabled={
                    enabled === null ||
                    isRecording ||
                    isStartingRecording ||
                    isStopping
                  }
                  onClick={() => void start()}
                >
                  {item.number ? "Start call" : "Start recording"}
                </Button>
              </div>
            )}
          </div>
        </TabsContent>
      </Tabs>
    </main>
  );
}
