'use client';

import { FolderOpen, HardDrive } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { voiceService, errorMessage } from '@/services/voiceService';
import { SettingsSection } from './SettingsParts';
import { useRecordingPreferences } from './RecordingTab';

/** Settings → General: where recordings are saved. */
export function GeneralTab() {
  const { preferences } = useRecordingPreferences();
  return (
    <SettingsSection
      title="Recordings folder"
      icon={<HardDrive className="w-5 h-5" />}
      description="Every recording is always saved, one folder per meeting with its audio.mp4 and transcripts."
    >
      <div className="rounded-md border border-gray-200 bg-gray-50 p-3 font-mono text-sm text-gray-700 break-all">
        {preferences ? preferences.save_folder || 'Default folder' : '…'}
      </div>
      <Button
        variant="outline"
        size="sm"
        onClick={() => voiceService.openRecordingsFolder().catch((e) => toast.error('Failed to open the folder', { description: errorMessage(e) }))}
      >
        <FolderOpen className="w-4 h-4 mr-2" />
        Open folder
      </Button>
    </SettingsSection>
  );
}
