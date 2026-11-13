"use client";

import { Sparkles, Cpu, FileText, Check, Square } from "lucide-react";

import { Button } from "@/components/ui/button";
import { ButtonGroup } from "@/components/ui/button-group";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ModelOption, SummaryTemplate } from "@/types/voice";

interface SummaryGeneratorButtonGroupProps {
  summaryModel: string | null;
  summaryModels: ModelOption[];
  onSummaryModelChange: (model: string) => void;
  onGenerateSummary: (customPrompt: string) => Promise<void>;
  onStopGeneration: () => void;
  customPrompt: string;
  summaryStatus:
    | "idle"
    | "processing"
    | "summarizing"
    | "regenerating"
    | "completed"
    | "error";
  availableTemplates: SummaryTemplate[];
  selectedTemplate: string;
  onTemplateSelect: (templateId: string, templateName: string) => void;
  hasTranscripts: boolean;
  hasSummary: boolean;
}

export function SummaryGeneratorButtonGroup({
  summaryModel,
  summaryModels,
  onSummaryModelChange,
  onGenerateSummary,
  onStopGeneration,
  customPrompt,
  summaryStatus,
  availableTemplates,
  selectedTemplate,
  onTemplateSelect,
  hasTranscripts,
  hasSummary,
}: SummaryGeneratorButtonGroupProps) {
  if (!hasTranscripts) {
    return null;
  }

  const isGenerating =
    summaryStatus === "processing" ||
    summaryStatus === "summarizing" ||
    summaryStatus === "regenerating";

  const modelLabel =
    summaryModels.find((m) => m.id === summaryModel)?.label ??
    summaryModel ??
    "Model";

  return (
    <ButtonGroup>
      {isGenerating ? (
        <Button
          variant="outline"
          size="sm"
          onClick={onStopGeneration}
          title="Stop summary generation"
        >
          <Square size={18} fill="currentColor" />
          <span className="hidden @[24rem]:inline">Stop</span>
        </Button>
      ) : (
        <Button
          variant="outline"
          size="sm"
          onClick={() => void onGenerateSummary(customPrompt)}
          title={hasSummary ? "Regenerate summary" : "Generate summary"}
        >
          <Sparkles size={18} />
          <span className="hidden @[24rem]:inline">
            {hasSummary ? "Regenerate Summary" : "Generate Summary"}
          </span>
        </Button>
      )}

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            title={`Summary model: ${modelLabel}`}
            disabled={isGenerating}
          >
            <Cpu />
            <span className="hidden @[40rem]:inline">{modelLabel}</span>
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuLabel>Summary model</DropdownMenuLabel>
          <DropdownMenuSeparator />
          {summaryModels.map((model) => (
            <DropdownMenuItem
              key={model.id}
              disabled={!model.available}
              onClick={() => onSummaryModelChange(model.id)}
              title={model.note}
              className="flex items-center justify-between"
            >
              <span>
                {model.label}
                {model.available ? "" : " (unavailable)"}
              </span>
              {summaryModel === model.id && (
                <Check className="h-4 w-4 text-success" />
              )}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>

      {availableTemplates.length > 0 && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm" title="Select summary template">
              <FileText />
              <span className="hidden @[40rem]:inline">Template</span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {availableTemplates.map((template) => (
              <DropdownMenuItem
                key={template.id}
                onClick={() => onTemplateSelect(template.id, template.name)}
                title={template.description}
                className="flex items-center justify-between"
              >
                <span>{template.name}</span>
                {selectedTemplate === template.id && (
                  <Check className="h-4 w-4 text-success" />
                )}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </ButtonGroup>
  );
}
