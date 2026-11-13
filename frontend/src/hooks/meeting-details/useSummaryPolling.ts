import React from "react";

import { voiceService } from "@/services/voiceService";
import type { SummaryProcessResponse } from "@/types";

interface SummaryPoll {
  processId: string;
  timer: ReturnType<typeof setInterval>;
  inFlight: boolean;
}

export function useSummaryPolling() {
  const summaryPollsRef = React.useRef(new Map<string, SummaryPoll>());

  // Summary polling management
  const stopSummaryPolling = React.useCallback(
    (meetingId: string, processId?: string) => {
      const poll = summaryPollsRef.current.get(meetingId);

      if (!poll || (processId && poll.processId !== processId)) {
        return;
      }

      clearInterval(poll.timer);
      summaryPollsRef.current.delete(meetingId);
    },
    []
  );

  const startSummaryPolling = React.useCallback(
    (
      meetingId: string,
      processId: string,
      onUpdate: (result: SummaryProcessResponse) => void | Promise<void>
    ) => {
      stopSummaryPolling(meetingId);
      let pollCount = 0;
      const maxPolls = 200;

      const poll = async () => {
        const entry = summaryPollsRef.current.get(meetingId);

        if (!entry || entry.processId !== processId || entry.inFlight) {
          return;
        }

        entry.inFlight = true;

        try {
          pollCount += 1;

          if (pollCount >= maxPolls) {
            await onUpdate({
              data: null,
              end: null,
              error: "Summary generation timed out. Please try again.",
              meetingName: null,
              meeting_id: meetingId,
              start: processId,
              status: "error",
            });

            if (summaryPollsRef.current.get(meetingId) === entry) {
              stopSummaryPolling(meetingId, processId);
            }

            return;
          }

          const result = await voiceService.getSummary(meetingId);
          const current = summaryPollsRef.current.get(meetingId);

          if (current !== entry || result.start !== processId) {
            return;
          }

          await onUpdate(result);

          if (summaryPollsRef.current.get(meetingId) !== entry) {
            return;
          }

          if (
            result.status === "completed" ||
            result.status === "error" ||
            result.status === "failed" ||
            result.status === "cancelled" ||
            (result.status === "idle" && pollCount > 1)
          ) {
            stopSummaryPolling(meetingId, processId);
          }
        } catch (error) {
          const current = summaryPollsRef.current.get(meetingId);

          if (current !== entry) {
            return;
          }

          try {
            await onUpdate({
              data: null,
              end: null,
              error: error instanceof Error ? error.message : "Unknown error",
              meetingName: null,
              meeting_id: meetingId,
              start: processId,
              status: "error",
            });
          } catch (callbackError) {
            console.error(
              "Failed to handle summary polling error:",
              callbackError
            );
          } finally {
            if (summaryPollsRef.current.get(meetingId) === entry) {
              stopSummaryPolling(meetingId, processId);
            }
          }
        } finally {
          const current = summaryPollsRef.current.get(meetingId);

          if (current === entry) {
            current.inFlight = false;
          }
        }
      };

      const timer = setInterval(() => void poll(), 5000);
      summaryPollsRef.current.set(meetingId, {
        inFlight: false,
        processId,
        timer,
      });
    },
    [stopSummaryPolling]
  );

  React.useEffect(
    () => () => {
      summaryPollsRef.current.forEach(({ timer }) => clearInterval(timer));
      summaryPollsRef.current.clear();
    },
    []
  );

  return { startSummaryPolling, stopSummaryPolling };
}
