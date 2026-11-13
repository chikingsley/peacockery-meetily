"use client";

import type { RefObject } from "react";

import type { BlockNoteSummaryViewRef } from "@/components/AISummary/BlockNoteSummaryView";
import { BlockNoteSummaryView } from "@/components/AISummary/BlockNoteSummaryView";
import { EmptyStateSummary } from "@/components/EmptyStateSummary";
import { hasVisibleSummaryContent } from "@/lib/summary-content";
import type { MeetingSummary } from "@/types";
import type { ModelOption, SummaryTemplate } from "@/types/voice";

import { SummaryGeneratorButtonGroup } from "./SummaryGeneratorButtonGroup";
import { SummaryUpdaterButtonGroup } from "./SummaryUpdaterButtonGroup";

type SummaryStatus =
  | "idle"
  | "processing"
  | "summarizing"
  | "regenerating"
  | "completed"
  | "error";

interface SummaryPanelProps {
  isSummaryDirty: boolean;
  summaryRef: RefObject<BlockNoteSummaryViewRef | null>;
  isSaving: boolean;
  onSaveAll: () => Promise<void>;
  onCopySummary: () => Promise<void>;
  aiSummary: MeetingSummary | null;
  summaryStatus: SummaryStatus;
  hasTranscripts: boolean;
  summaryModel: string | null;
  summaryModels: ModelOption[];
  onSummaryModelChange: (model: string) => void;
  onGenerateSummary: (customPrompt: string) => Promise<void>;
  onStopGeneration: () => void;
  customPrompt: string;
  onSaveSummary: (summary: MeetingSummary) => Promise<void>;
  onDirtyChange: (isDirty: boolean) => void;
  summaryError: string | null;
  getSummaryStatusMessage: (status: SummaryStatus) => string;
  availableTemplates: SummaryTemplate[];
  selectedTemplate: string;
  onTemplateSelect: (templateId: string, templateName: string) => void;
}

export function SummaryPanel({
  isSummaryDirty,
  summaryRef,
  isSaving,
  onSaveAll,
  onCopySummary,
  aiSummary,
  summaryStatus,
  hasTranscripts,
  summaryModel,
  summaryModels,
  onSummaryModelChange,
  onGenerateSummary,
  onStopGeneration,
  customPrompt,
  onSaveSummary,
  onDirtyChange,
  summaryError,
  getSummaryStatusMessage,
  availableTemplates,
  selectedTemplate,
  onTemplateSelect,
}: SummaryPanelProps) {
  const isSummaryLoading =
    summaryStatus === "processing" ||
    summaryStatus === "summarizing" ||
    summaryStatus === "regenerating";

  const hasSummary = hasVisibleSummaryContent(aiSummary);

  return (
    <div className="@container flex h-full w-full min-w-0 flex-1 flex-col overflow-hidden bg-background">
      <div className="border-b border-border p-4">
        <div className="flex w-full min-w-0 flex-wrap items-center justify-center gap-2">
          <div className="min-w-0 flex-shrink-0">
            <SummaryGeneratorButtonGroup
              summaryModel={summaryModel}
              summaryModels={summaryModels}
              onSummaryModelChange={onSummaryModelChange}
              onGenerateSummary={onGenerateSummary}
              onStopGeneration={onStopGeneration}
              customPrompt={customPrompt}
              summaryStatus={summaryStatus}
              availableTemplates={availableTemplates}
              selectedTemplate={selectedTemplate}
              onTemplateSelect={onTemplateSelect}
              hasTranscripts={hasTranscripts}
              hasSummary={hasSummary}
            />
          </div>

          {hasSummary && !isSummaryLoading && (
            <div className="flex-shrink-0">
              <SummaryUpdaterButtonGroup
                isSaving={isSaving}
                isDirty={isSummaryDirty}
                onSave={onSaveAll}
                onCopy={onCopySummary}
              />
            </div>
          )}
        </div>
      </div>

      {isSummaryLoading ? (
        <div className="flex flex-1 items-center justify-center">
          <div className="text-center">
            <div className="mb-4 inline-block h-12 w-12 animate-spin rounded-full border-t-2 border-b-2 border-info/30" />
            <p className="text-muted-foreground">Generating summary...</p>
          </div>
        </div>
      ) : hasSummary ? (
        <div className="min-h-0 flex-1 overflow-x-auto overflow-y-auto">
          <div className="mx-auto w-full max-w-3xl p-4">
            <BlockNoteSummaryView
              ref={summaryRef}
              summaryData={aiSummary}
              onSave={onSaveSummary}
              onDirtyChange={onDirtyChange}
            />
          </div>
          {summaryStatus !== "idle" && (
            <div
              className={`mt-4 rounded-lg p-4 ${
                summaryStatus === "error"
                  ? "bg-destructive/10 text-destructive"
                  : summaryStatus === "completed"
                    ? "bg-success/10 text-success"
                    : "bg-info/10 text-info"
              }`}
            >
              <p className="text-sm font-medium">
                {getSummaryStatusMessage(summaryStatus)}
              </p>
            </div>
          )}
        </div>
      ) : (
        <EmptyStateSummary error={summaryError} />
      )}
    </div>
  );
}
