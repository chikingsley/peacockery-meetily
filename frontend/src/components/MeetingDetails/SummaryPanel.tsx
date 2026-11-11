"use client";

import type { RefObject } from 'react';
import type { MeetingSummary } from '@/types';
import type { ModelOption, SummaryTemplate } from '@/types/voice';
import { BlockNoteSummaryView, BlockNoteSummaryViewRef } from '@/components/AISummary/BlockNoteSummaryView';
import { EmptyStateSummary } from '@/components/EmptyStateSummary';
import { SummaryGeneratorButtonGroup } from './SummaryGeneratorButtonGroup';
import { SummaryUpdaterButtonGroup } from './SummaryUpdaterButtonGroup';
import { hasVisibleSummaryContent } from '@/lib/summary-content';

type SummaryStatus = 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';

interface SummaryPanelProps {
  isSummaryDirty: boolean;
  summaryRef: RefObject<BlockNoteSummaryViewRef>;
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
  const isSummaryLoading = summaryStatus === 'processing' || summaryStatus === 'summarizing' || summaryStatus === 'regenerating';
  const hasSummary = hasVisibleSummaryContent(aiSummary);

  return (
    <div className="flex-1 min-w-0 flex flex-col bg-white overflow-hidden h-full w-full @container">
      <div className="p-4 border-b border-gray-200">
        <div className="flex items-center justify-center w-full min-w-0 gap-2 flex-wrap">
          <div className="flex-shrink-0 min-w-0">
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
        <div className="flex items-center justify-center flex-1">
          <div className="text-center">
            <div className="inline-block animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-blue-500 mb-4"></div>
            <p className="text-gray-600">Generating summary...</p>
          </div>
        </div>
      ) : !hasSummary ? (
        <EmptyStateSummary
          onGenerate={() => onGenerateSummary(customPrompt)}
          hasModel={!!summaryModel}
          isGenerating={isSummaryLoading}
          error={summaryError}
        />
      ) : (
        <div className="flex-1 overflow-y-auto overflow-x-auto min-h-0">
          <div className="p-6 w-full">
            <BlockNoteSummaryView
              ref={summaryRef}
              summaryData={aiSummary}
              onSave={onSaveSummary}
              onDirtyChange={onDirtyChange}
            />
          </div>
          {summaryStatus !== 'idle' && (
            <div className={`mt-4 p-4 rounded-lg ${summaryStatus === 'error' ? 'bg-red-100 text-red-700' :
              summaryStatus === 'completed' ? 'bg-green-100 text-green-700' :
                'bg-blue-100 text-blue-700'
              }`}>
              <p className="text-sm font-medium">{getSummaryStatusMessage(summaryStatus)}</p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
