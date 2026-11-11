import { toast } from 'sonner';
import { AudioLines } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { useConfig } from '@/contexts/ConfigContext';
import { errorMessage } from '@/services/voiceService';
import { LANGUAGES, languageLabel, modelChoices, modelLabel, modelOptionText } from '@/lib/voice-models';
import type { VoiceSettings } from '@/types/voice';

/**
 * Live model and language for the next recording, right on the home screen. While recording it
 * shows what the current session uses (the choice is fixed per recording).
 */
export function LiveQuickSettings({ isRecording }: { isRecording: boolean }) {
  const { voiceSettings, saveVoiceSettings, voiceModels } = useConfig();
  if (!voiceSettings) return null;

  const liveOptions = modelChoices(voiceModels?.live ?? [], voiceSettings.live_model);
  const update = (patch: Partial<VoiceSettings>) => {
    saveVoiceSettings({ ...voiceSettings, ...patch })
      .catch((error) => toast.error('Failed to save settings', { description: errorMessage(error) }));
  };

  if (isRecording || !voiceSettings.live_enabled) {
    return (
      <div className="flex items-center gap-1.5 text-xs text-gray-600">
        <AudioLines className="h-3.5 w-3.5" />
        {voiceSettings.live_enabled
          ? <>Live: <span className="font-medium">{modelLabel(voiceModels?.live, voiceSettings.live_model)}</span> · {languageLabel(voiceSettings.language)}</>
          : 'Live transcript off'}
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <AudioLines className="h-4 w-4 text-gray-500" />
      <Select value={voiceSettings.live_model} onValueChange={(live_model) => update({ live_model })}>
        <SelectTrigger className="h-8 w-[230px] text-xs" aria-label="Live model">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {liveOptions.map(o => (
            <SelectItem key={o.id} value={o.id} disabled={!o.available} className="text-xs">
              {modelOptionText(o)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select value={voiceSettings.language} onValueChange={(language) => update({ language })}>
        <SelectTrigger className="h-8 w-[150px] text-xs" aria-label="Language">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {LANGUAGES.map(l => <SelectItem key={l.code} value={l.code} className="text-xs">{l.label}</SelectItem>)}
          {!LANGUAGES.some(l => l.code === voiceSettings.language) && (
            <SelectItem value={voiceSettings.language} className="text-xs">{voiceSettings.language}</SelectItem>
          )}
        </SelectContent>
      </Select>
    </div>
  );
}
