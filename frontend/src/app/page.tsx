'use client';

import { useCallback, useState } from 'react';
import { RecordingControls } from '@/components/RecordingControls';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useRecordingStart } from '@/hooks/useRecordingStart';
import { TranscriptPanel } from './_components/TranscriptPanel';
import { VoiceNotConfiguredDialog } from './_components/VoiceNotConfiguredDialog';

export default function Home() {
  const [showVoiceDialog, setShowVoiceDialog] = useState(false);
  const { isCollapsed: sidebarCollapsed } = useSidebar();
  const onVoiceNotConfigured = useCallback(() => setShowVoiceDialog(true), []);
  const { startRecording } = useRecordingStart(onVoiceNotConfigured);

  return (
    <div className="flex flex-col h-screen bg-gray-50">
      <VoiceNotConfiguredDialog open={showVoiceDialog} onOpenChange={setShowVoiceDialog} />
      <div className="flex flex-1 overflow-hidden">
        <TranscriptPanel />
        <div className="fixed bottom-12 left-0 right-0 z-10">
          <div
            className="flex justify-center pl-8 transition-[margin] duration-300"
            style={{ marginLeft: sidebarCollapsed ? '4rem' : '16rem' }}
          >
            <div className="w-2/3 max-w-[750px] flex justify-center">
              <RecordingControls onStart={startRecording} />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
