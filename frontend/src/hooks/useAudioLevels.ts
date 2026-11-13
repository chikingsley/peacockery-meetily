import { useEffect, useState } from "react";

import { voiceService } from "@/services/voiceService";
import type { AudioLevels } from "@/types/voice";

const SILENT: AudioLevels = { mic: 0, system: 0, system_silent_seconds: 0 };

/** Latest `audio-levels` payload while `active`; zeros otherwise. */
export function useAudioLevels(active: boolean): AudioLevels {
  const [levels, setLevels] = useState<AudioLevels>(SILENT);
  useEffect(() => {
    if (!active) {
      setLevels(SILENT);

      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | undefined;
    voiceService
      .onAudioLevels(setLevels)
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
  }, [active]);

  return levels;
}
