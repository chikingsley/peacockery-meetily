"use client";

import { motion } from "framer-motion";
import { useState, useEffect, useRef } from "react";
import { toast } from "sonner";

import { MeetingDetailsSplitView } from "@/components/MeetingDetails/MeetingDetailsSplitView";
import type { MeetingDetailsTab } from "@/components/MeetingDetails/MeetingDetailsSplitView";
import { SummaryPanel } from "@/components/MeetingDetails/SummaryPanel";
import { TranscriptPanel } from "@/components/MeetingDetails/TranscriptPanel";
import { useConfig } from "@/contexts/ConfigContext";
import { useCopyOperations } from "@/hooks/meeting-details/useCopyOperations";
import { useFinalTranscript } from "@/hooks/meeting-details/useFinalTranscript";
import { useMeetingData } from "@/hooks/meeting-details/useMeetingData";
import { useSummaryGeneration } from "@/hooks/meeting-details/useSummaryGeneration";
import { useTemplates } from "@/hooks/meeting-details/useTemplates";
import { voiceService, errorMessage } from "@/services/voiceService";
import type { MeetingSummary, SummaryProcessResponse } from "@/types";
import type {
  MeetingMetadata,
  Transcript,
  TranscriptSource,
} from "@/types/voice";

export default function PageContent({
  meeting,
  summaryData,
  initialSummary,
  onMeetingUpdated,
  onFinalTranscriptDone,
  onRenameSpeaker,
  justRecorded,
  transcripts,
  source,
  showingLiveDraft,
  onToggleSource,
  hasMore,
  isLoadingMore,
  totalCount,
  onLoadMore,
}: {
  meeting: MeetingMetadata;
  summaryData: MeetingSummary | null;
  initialSummary: SummaryProcessResponse | null;
  onMeetingUpdated?: () => Promise<void>;
  onFinalTranscriptDone: () => Promise<void>;
  onRenameSpeaker: (from: string, to: string) => Promise<void>;
  justRecorded: boolean;
  transcripts: Transcript[];
  source: TranscriptSource | null;
  showingLiveDraft: boolean;
  onToggleSource: () => void;
  hasMore: boolean;
  isLoadingMore: boolean;
  totalCount: number;
  onLoadMore: () => void;
}) {
  const [customPrompt, setCustomPrompt] = useState<string>("");
  const [activeTab, setActiveTab] = useState<MeetingDetailsTab>("transcript");
  const autoSwitchedSummaryMeetingIdsRef = useRef(new Set<string>());
  const manuallySelectedTabMeetingIdsRef = useRef(new Set<string>());

  const { voiceSettings, voiceModels } = useConfig();
  // Per-meeting override of the summary model; defaults to the Voice settings choice.
  const [summaryModel, setSummaryModel] = useState<string | null>(null);

  const effectiveSummaryModel =
    summaryModel ?? voiceSettings?.summary_model ?? null;

  const meetingData = useMeetingData({ meeting, summaryData });
  const templates = useTemplates();
  const finalTranscript = useFinalTranscript(meeting, onFinalTranscriptDone);

  const summaryGeneration = useSummaryGeneration({
    initialSummary,
    meeting,
    onMeetingUpdated,
    selectedTemplate: templates.selectedTemplate,
    setAiSummary: meetingData.setAiSummary,
    summaryModel: effectiveSummaryModel,
    updateMeetingTitle: meetingData.updateMeetingTitle,
  });

  const copyOperations = useCopyOperations({
    aiSummary: meetingData.aiSummary,
    blockNoteSummaryRef: meetingData.blockNoteSummaryRef,
    meeting,
    meetingTitle: meetingData.meetingTitle,
    transcriptSource: showingLiveDraft ? "live" : "best",
  });

  const handleOpenMeetingFolder = () => {
    voiceService.openMeetingFolder(meeting.id).catch((error) => {
      toast.error("Failed to open recording folder", {
        description: errorMessage(error),
      });
    });
  };

  useEffect(() => {
    if (
      (meetingData.aiSummary ||
        summaryGeneration.summaryStatus === "completed") &&
      !autoSwitchedSummaryMeetingIdsRef.current.has(meeting.id) &&
      !manuallySelectedTabMeetingIdsRef.current.has(meeting.id)
    ) {
      autoSwitchedSummaryMeetingIdsRef.current.add(meeting.id);
      setActiveTab("summary");
    }
  }, [meeting.id, meetingData.aiSummary, summaryGeneration.summaryStatus]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: "easeOut" }}
      className="flex h-full min-w-0 flex-col bg-muted/40"
    >
      <div className="flex min-w-0 flex-1 overflow-hidden">
        <MeetingDetailsSplitView
          activeTab={activeTab}
          onTabChange={(tab) => {
            manuallySelectedTabMeetingIdsRef.current.add(meeting.id);
            setActiveTab(tab);
          }}
          transcript={
            <TranscriptPanel
              meeting={meeting}
              transcripts={transcripts}
              source={source}
              justRecorded={justRecorded}
              onToggleSource={onToggleSource}
              onRenameSpeaker={onRenameSpeaker}
              finalTranscript={finalTranscript}
              customPrompt={customPrompt}
              onPromptChange={setCustomPrompt}
              onCopyTranscript={() =>
                void copyOperations.handleCopyTranscript()
              }
              onOpenMeetingFolder={handleOpenMeetingFolder}
              onStartFinalTranscript={(model) =>
                void finalTranscript.start(model)
              }
              hasMore={hasMore}
              isLoadingMore={isLoadingMore}
              totalCount={totalCount}
              onLoadMore={onLoadMore}
            />
          }
          summary={
            <SummaryPanel
              summaryRef={meetingData.blockNoteSummaryRef}
              isSaving={meetingData.isSaving}
              isSummaryDirty={meetingData.isSummaryDirty}
              onSaveAll={meetingData.saveAllChanges}
              onCopySummary={copyOperations.handleCopySummary}
              aiSummary={meetingData.aiSummary}
              summaryStatus={summaryGeneration.summaryStatus}
              hasTranscripts={totalCount > 0}
              summaryModel={effectiveSummaryModel}
              summaryModels={voiceModels?.summary ?? []}
              onSummaryModelChange={setSummaryModel}
              onGenerateSummary={summaryGeneration.handleGenerateSummary}
              onStopGeneration={summaryGeneration.handleStopGeneration}
              customPrompt={customPrompt}
              onSaveSummary={meetingData.handleSaveSummary}
              onDirtyChange={meetingData.setIsSummaryDirty}
              summaryError={summaryGeneration.summaryError}
              getSummaryStatusMessage={
                summaryGeneration.getSummaryStatusMessage
              }
              availableTemplates={templates.availableTemplates}
              selectedTemplate={templates.selectedTemplate}
              onTemplateSelect={templates.handleTemplateSelection}
            />
          }
        />
      </div>
    </motion.div>
  );
}
