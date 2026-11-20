"use client";

import { useState } from "react";
import { toast } from "sonner";

import { Input } from "@/components/ui/input";
import { useMeetings } from "@/contexts/MeetingContext";
import { errorMessage, voiceService } from "@/services/voiceService";

/** The open meeting's title in the header: click it to rename; Enter saves, Escape cancels. */
export function MeetingTitle() {
  const { currentMeeting, setCurrentMeeting, refetchMeetings } = useMeetings();
  const [draft, setDraft] = useState<string | null>(null);

  if (!currentMeeting) {
    return <span className="truncate text-sm font-medium">Meeting</span>;
  }

  const save = async () => {
    const title = draft?.trim();
    setDraft(null);

    if (!title || title === currentMeeting.title) {
      return;
    }

    try {
      await voiceService.saveMeetingTitle(currentMeeting.id, title);
      setCurrentMeeting({ id: currentMeeting.id, title });
      await refetchMeetings();
    } catch (error) {
      toast.error("Rename failed", { description: errorMessage(error) });
    }
  };

  if (draft === null) {
    return (
      <button
        type="button"
        title="Rename this meeting"
        className="truncate rounded px-1 text-left text-sm font-medium hover:bg-muted"
        onClick={() => setDraft(currentMeeting.title)}
      >
        {currentMeeting.title}
      </button>
    );
  }

  return (
    <Input
      autoFocus
      aria-label="Meeting title"
      className="h-8 max-w-md"
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => void save()}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.currentTarget.blur();
        } else if (event.key === "Escape") {
          setDraft(null);
        }
      }}
    />
  );
}
