"use client";

import { Loader2, UserPen } from "lucide-react";
import { useEffect, useState } from "react";

import { SpeakerChip } from "@/components/SpeakerChip";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

interface SpeakerRenameDialogProps {
  /** Speaker being renamed; null closes the dialog. */
  speaker: string | null;
  /** Names to offer as one-click choices (labels from Settings and the meeting's other speakers). */
  suggestions: string[];
  onClose: () => void;
  onRename: (from: string, to: string) => Promise<void>;
}

/** Rename one speaker across this meeting's transcript; picking an existing name merges them. */
export function SpeakerRenameDialog({
  speaker,
  suggestions,
  onClose,
  onRename,
}: SpeakerRenameDialogProps) {
  const [name, setName] = useState("");
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (speaker) {
      setName(speaker);
    }
  }, [speaker]);

  const submit = async (to: string) => {
    const target = to.trim();

    if (!speaker || !target || target === speaker) {
      return;
    }

    setSaving(true);

    try {
      await onRename(speaker, target);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const choices = suggestions.filter((s) => s !== speaker);

  return (
    <Dialog
      open={speaker !== null}
      onOpenChange={(open) => {
        if (!open && !saving) {
          onClose();
        }
      }}
    >
      <DialogContent className="sm:max-w-[420px]">
        <DialogHeader>
          <DialogTitle className="flex items-center">
            <UserPen className="h-5 w-5 text-info" />
            Rename speaker
          </DialogTitle>
          <DialogDescription>
            Every line by {speaker ? <SpeakerChip speaker={speaker} /> : null}{" "}
            in this meeting gets the new name.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            void submit(name);
          }}
        >
          <Input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            aria-label="Speaker name"
          />
          {choices.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {choices.map((choice) => (
                <button
                  type="button"
                  key={choice}
                  disabled={saving}
                  onClick={() => setName(choice)}
                  className="rounded-full border border-border px-2.5 py-0.5 text-xs text-foreground hover:bg-muted/40"
                >
                  {choice}
                </button>
              ))}
            </div>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={saving}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={saving || !name.trim() || name.trim() === speaker}
            >
              {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              Rename
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
