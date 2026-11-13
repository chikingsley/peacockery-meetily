"use client";

import { AudioLines } from "lucide-react";
import { useEffect, useState } from "react";

import { ModelSelect } from "@/components/settings/SettingsParts";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useConfig } from "@/contexts/ConfigContext";

interface FinalTranscriptDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hasFinal: boolean;
  onStart: (model: string | null) => void;
}

/** (Re)run the final transcript of a meeting with a chosen batch model. */
export function FinalTranscriptDialog({
  open,
  onOpenChange,
  hasFinal,
  onStart,
}: FinalTranscriptDialogProps) {
  const { voiceSettings, voiceModels } = useConfig();
  const [model, setModel] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setModel(voiceSettings?.final_model ?? null);
    }
  }, [open, voiceSettings?.final_model]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle className="flex items-center">
            <AudioLines className="h-5 w-5 text-info" />
            {hasFinal ? "Re-run final transcript" : "Run final transcript"}
          </DialogTitle>
          <DialogDescription>
            Uploads this meeting&apos;s recording to Peacockery Voice and
            replaces the final transcript. The live transcript is kept.
          </DialogDescription>
        </DialogHeader>
        {model !== null && (
          <ModelSelect
            id="final-run-model"
            label="Final model"
            value={model}
            options={voiceModels?.final ?? []}
            onChange={setModel}
          />
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            onClick={() => {
              onStart(model);
              onOpenChange(false);
            }}
          >
            Start
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
