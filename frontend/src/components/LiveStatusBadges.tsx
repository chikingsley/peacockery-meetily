import { Loader2 } from "lucide-react";

import { useConfig } from "@/contexts/ConfigContext";
import { byChannel, CHANNEL_LABEL } from "@/lib/live-transcript";
import type { StatusByChannel } from "@/lib/live-transcript";
import { modelLabel } from "@/lib/voice-models";
import type { LiveTranscriptionStatus } from "@/types/voice";

const STATE_CLASS: Record<LiveTranscriptionStatus["state"], string> = {
  closed: "bg-muted/40 text-muted-foreground",
  connecting: "bg-muted/40 text-foreground",
  failed: "bg-destructive/10 text-destructive",
  live: "bg-success/10 text-success",
  reconnecting: "bg-warning/10 text-warning",
};

/** One small pill per live session (`live-transcription-status`): channel, state and model. */
export function LiveStatusBadges({
  liveStatus,
}: {
  liveStatus: StatusByChannel;
}) {
  const { voiceModels } = useConfig();
  const entries = byChannel(liveStatus);

  if (entries.length === 0) {
    return null;
  }

  return (
    <div className="flex flex-wrap gap-1.5">
      {entries.map((s) => {
        const model = modelLabel(voiceModels?.live, s.model);

        return (
          <span
            key={s.channel}
            title={s.message ?? `${model}: ${s.state}`}
            className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${STATE_CLASS[s.state]}`}
          >
            {(s.state === "connecting" || s.state === "reconnecting") && (
              <Loader2 className="h-3 w-3 animate-spin" />
            )}
            {s.state === "live" && (
              <span className="h-1.5 w-1.5 rounded-full bg-success" />
            )}
            {CHANNEL_LABEL[s.channel]} · {s.state}
            <span className="font-normal opacity-75">· {model}</span>
          </span>
        );
      })}
    </div>
  );
}
