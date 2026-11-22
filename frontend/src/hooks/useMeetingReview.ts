import { useCallback, useEffect, useRef, useState } from "react";

import { voiceService } from "@/services/voiceService";

type Review = Awaited<ReturnType<typeof voiceService.meetingReview>>;

const empty: Review = { names: {}, outcomes: [] };

export function useMeetingReview(meetingId: string | null) {
  const [result, setResult] = useState<{ id: string; value: Review } | null>(
    null
  );

  const generation = useRef(0);

  const refresh = useCallback(async () => {
    if (!meetingId) {
      return;
    }

    const version = ++generation.current;

    try {
      const value = await voiceService.meetingReview(meetingId);

      if (version === generation.current) {
        setResult({ id: meetingId, value });
      }
    } catch (error) {
      console.warn("Meeting review unavailable", error);
    }
  }, [meetingId]);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 30_000);

    return () => {
      clearInterval(timer);
    };
  }, [refresh]);

  return { review: result?.id === meetingId ? result.value : empty, refresh };
}
