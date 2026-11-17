"use client";

import { Mic, SlidersHorizontal } from "lucide-react";
import { useCallback, useState } from "react";

import { DeviceSelection } from "@/components/DeviceSelection";
import { RecordingControls } from "@/components/RecordingControls";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useConfig } from "@/contexts/ConfigContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { useDesktopRuntime } from "@/hooks/useDesktopRuntime";
import { useRecordingStart } from "@/hooks/useRecordingStart";
import { suggestedMode } from "@/lib/recording-context";
import type { CalendarEvent, RecordingMode } from "@/types/voice";

import { LiveQuickSettings } from "./_components/LiveQuickSettings";
import { RecordingSetup } from "./_components/RecordingSetup";
import { TranscriptPanel } from "./_components/TranscriptPanel";
import { VoiceNotConfiguredDialog } from "./_components/VoiceNotConfiguredDialog";

export default function Home() {
  const desktop = useDesktopRuntime();
  const [showVoiceDialog, setShowVoiceDialog] = useState(false);
  const [activationDismissed, setActivationDismissed] = useState(false);
  const [title, setTitle] = useState("");
  const [mode, setMode] = useState<RecordingMode>("in_person");

  const [calendarEvent, setCalendarEvent] = useState<CalendarEvent | null>(
    null
  );

  const selectEvent = (event: CalendarEvent | null) => {
    setCalendarEvent(event);

    if (event) {
      setTitle(event.title);
      setMode(suggestedMode(event));
    }
  };

  const { isRecording, isStartingRecording, isStopping } = useRecordingState();
  const { selectedDevices, setSelectedDevices, keyStatus } = useConfig();
  const onVoiceNotConfigured = useCallback(() => setShowVoiceDialog(true), []);
  const { startRecording } = useRecordingStart(onVoiceNotConfigured);
  const active = isRecording || isStartingRecording || isStopping;

  return (
    <div className="flex h-full flex-col bg-background">
      <VoiceNotConfiguredDialog
        open={
          showVoiceDialog ||
          (desktop && keyStatus?.configured === false && !activationDismissed)
        }
        onOpenChange={(open) => {
          setShowVoiceDialog(open);

          if (!open) {
            setActivationDismissed(true);
          }
        }}
      />
      {active ? (
        <>
          <div className="flex min-h-0 flex-1 overflow-hidden">
            <TranscriptPanel />
          </div>
          <div className="flex shrink-0 justify-center border-t p-4">
            <RecordingControls
              onStart={() => startRecording(title, mode, calendarEvent)}
            />
          </div>
        </>
      ) : (
        <div className="flex-1 overflow-y-auto">
          <div className="mx-auto max-w-xl px-5 py-8">
            <form
              className="space-y-5"
              onSubmit={(event) => {
                event.preventDefault();

                if (desktop) {
                  void startRecording(title, mode, calendarEvent);
                }
              }}
            >
              <RecordingSetup
                mode={mode}
                onModeChange={setMode}
                event={calendarEvent}
                onEventChange={selectEvent}
              />
              <div className="space-y-2">
                <Label htmlFor="meeting-title">Meeting title (optional)</Label>
                <Input
                  id="meeting-title"
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  placeholder="Untitled meeting"
                />
              </div>
              <div className="flex flex-wrap items-center gap-4">
                <Button type="submit" disabled={active || !desktop}>
                  <Mic />
                  Start recording
                </Button>
                <p className="text-xs text-muted-foreground">
                  Separates speakers in the live transcript and the final
                  transcript.
                </p>
              </div>
              <details className="rounded-lg border p-4">
                <summary className="flex cursor-pointer items-center gap-2 text-sm font-medium">
                  <SlidersHorizontal className="size-4" />
                  Recording options
                </summary>
                <div className="mt-4 space-y-4">
                  <DeviceSelection
                    selectedDevices={selectedDevices}
                    onDeviceChange={setSelectedDevices}
                  />
                  <LiveQuickSettings isRecording={false} />
                </div>
              </details>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
