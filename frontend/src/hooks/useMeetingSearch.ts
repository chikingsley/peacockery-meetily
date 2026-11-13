import React, { useEffect, useState } from "react";

import { voiceService } from "@/services/voiceService";
import type { TranscriptSearchResult } from "@/types/voice";

export function useMeetingSearch(
  search = voiceService.searchTranscripts,
  delay = 200
) {
  const [searchResults, setSearchResults] = useState<TranscriptSearchResult[]>(
    []
  );

  const [isSearching, setIsSearching] = useState(false);
  // Function to search through meeting transcripts
  const searchEpoch = React.useRef(0);
  const searchTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      searchEpoch.current += 1;

      if (searchTimer.current) {
        clearTimeout(searchTimer.current);
      }
    },
    []
  );

  const searchTranscripts = React.useCallback(
    async (query: string) => {
      const epoch = ++searchEpoch.current;

      if (searchTimer.current) {
        clearTimeout(searchTimer.current);
      }

      setSearchResults([]);
      setIsSearching(false);

      if (!query.trim()) {
        setSearchResults([]);

        return;
      }

      setIsSearching(true);
      searchTimer.current = setTimeout(async () => {
        try {
          const results = await search(query);

          if (epoch === searchEpoch.current) {
            setSearchResults(results);
          }
        } catch (error) {
          console.error("Error searching transcripts:", error);

          if (epoch === searchEpoch.current) {
            setSearchResults([]);
          }
        } finally {
          if (epoch === searchEpoch.current) {
            setIsSearching(false);
          }
        }
      }, delay);
    },
    [search, delay]
  );

  return { searchTranscripts, searchResults, isSearching };
}
