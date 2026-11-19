"use client";

import { usePathname, useRouter } from "next/navigation";
import React, { createContext, useContext, useState, useEffect } from "react";

import { useRecordingState } from "@/contexts/RecordingStateContext";
import { useSummaryPolling } from "@/hooks/meeting-details/useSummaryPolling";
import { useMeetingSearch } from "@/hooks/useMeetingSearch";
import { voiceService } from "@/services/voiceService";
import type { SummaryProcessResponse } from "@/types";
import type { MeetingListItem, TranscriptSearchResult } from "@/types/voice";

export interface CurrentMeeting {
  id: string;
  title: string;
}

interface MeetingContextType {
  currentMeeting: CurrentMeeting | null;
  setCurrentMeeting: (meeting: CurrentMeeting | null) => void;
  meetings: MeetingListItem[];
  setMeetings: (meetings: MeetingListItem[]) => void;
  searchTranscripts: (query: string) => Promise<void>;
  searchResults: TranscriptSearchResult[];
  isSearching: boolean;
  // Summary polling management
  startSummaryPolling: (
    meetingId: string,
    processId: string,
    onUpdate: (result: SummaryProcessResponse) => void | Promise<void>
  ) => void;
  stopSummaryPolling: (meetingId: string, processId?: string) => void;
  // Refetch meetings from backend
  refetchMeetings: () => Promise<void>;
}

const MeetingContext = createContext<MeetingContextType | null>(null);

export const useMeetings = () => {
  const context = useContext(MeetingContext);

  if (!context) {
    throw new Error("useMeetings must be used within a MeetingProvider");
  }

  return context;
};

export function MeetingProvider({ children }: { children: React.ReactNode }) {
  const [currentMeeting, setCurrentMeeting] = useState<CurrentMeeting | null>(
    null
  );

  const [meetings, setMeetings] = useState<MeetingListItem[]>([]);
  const meetingsRef = React.useRef(meetings);
  meetingsRef.current = meetings;

  // Use recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();

  const pathname = usePathname();
  const router = useRouter();

  const fetchMeetings = React.useCallback(async () => {
    try {
      const list = await voiceService.getMeetings();
      setMeetings(list);
    } catch (error) {
      console.error("Error fetching meetings:", error);
      setMeetings([]);
    }
  }, []);

  React.useEffect(() => {
    fetchMeetings();
  }, [fetchMeetings]);

  // Keep each meeting's final-transcript indicator current; a new meeting (import, stop) refetches.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    voiceService
      .onFinalTranscriptStatus(({ meeting_id, state }) => {
        if (!meetingsRef.current.some((m) => m.id === meeting_id)) {
          void fetchMeetings();

          return;
        }

        setMeetings((prev) =>
          prev.map((m) =>
            m.id === meeting_id ? { ...m, final_status: state } : m
          )
        );
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((error) =>
        console.error("Native event synchronization failed", error)
      );

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [fetchMeetings]);

  useEffect(() => {
    if (pathname === "/") {
      setCurrentMeeting(null);
    }
  }, [pathname]);

  // Tray "Start Recording" opens recording setup; capture starts from there.
  const openRecordingSetup = () => {
    if (!isRecording) {
      router.push("/");
    }
  };

  const openRecordingSetupRef = React.useRef(openRecordingSetup);
  openRecordingSetupRef.current = openRecordingSetup;
  useEffect(() => {
    const unlisten = voiceService.onOpenRecordingSetup(() =>
      openRecordingSetupRef.current()
    );

    return () => {
      unlisten
        .then((fn) => fn())
        .catch((error) =>
          console.error("Native event synchronization failed", error)
        );
    };
  }, []);

  const { searchTranscripts, searchResults, isSearching } = useMeetingSearch();

  const { startSummaryPolling, stopSummaryPolling } = useSummaryPolling();

  return (
    <MeetingContext.Provider
      value={{
        currentMeeting,
        isSearching,
        meetings,
        refetchMeetings: fetchMeetings,
        searchResults,
        searchTranscripts,
        setCurrentMeeting,
        setMeetings,
        startSummaryPolling,
        stopSummaryPolling,
      }}
    >
      {children}
    </MeetingContext.Provider>
  );
}
