import { Loader2 } from 'lucide-react';
import type { LiveTranscriptionStatus } from '@/types/voice';
import { byChannel, CHANNEL_LABEL, type StatusByChannel } from '@/lib/live-transcript';
import { modelLabel } from '@/lib/voice-models';
import { useConfig } from '@/contexts/ConfigContext';

const STATE_CLASS: Record<LiveTranscriptionStatus['state'], string> = {
  connecting: 'bg-gray-100 text-gray-700',
  live: 'bg-green-100 text-green-800',
  reconnecting: 'bg-amber-100 text-amber-800',
  failed: 'bg-red-100 text-red-800',
  closed: 'bg-gray-100 text-gray-500',
};

/** One small pill per live session (`live-transcription-status`): channel, state and model. */
export function LiveStatusBadges({ liveStatus }: { liveStatus: StatusByChannel }) {
  const { voiceModels } = useConfig();
  const entries = byChannel(liveStatus);
  if (entries.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-1.5">
      {entries.map(s => {
        const model = modelLabel(voiceModels?.live, s.model);
        return (
          <span
            key={s.channel}
            title={s.message ?? `${model}: ${s.state}`}
            className={`inline-flex items-center gap-1 text-xs font-medium rounded-full px-2 py-0.5 ${STATE_CLASS[s.state]}`}
          >
            {(s.state === 'connecting' || s.state === 'reconnecting') && <Loader2 className="h-3 w-3 animate-spin" />}
            {s.state === 'live' && <span className="h-1.5 w-1.5 rounded-full bg-green-500" />}
            {CHANNEL_LABEL[s.channel]} · {s.state}
            <span className="font-normal opacity-75">· {model}</span>
          </span>
        );
      })}
    </div>
  );
}
