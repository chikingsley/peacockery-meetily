import { speakerColorClass } from '@/lib/speaker-colors';
import { useConfig } from '@/contexts/ConfigContext';

/** Coloured speaker label; "Me" is always blue, every other speaker gets its own colour. */
export function SpeakerChip({ speaker, onClick, title }: {
  speaker: string;
  onClick?: () => void;
  title?: string;
}) {
  const { voiceSettings } = useConfig();
  const className = `inline-block text-xs font-semibold rounded px-1.5 py-0.5 not-italic ${speakerColorClass(speaker, voiceSettings?.me_label)}`;
  if (!onClick) return <span className={className}>{speaker}</span>;
  return (
    <button
      type="button"
      onClick={(e) => { e.stopPropagation(); onClick(); }}
      title={title}
      className={`${className} hover:ring-2 hover:ring-offset-1 hover:ring-gray-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500`}
    >
      {speaker}
    </button>
  );
}
