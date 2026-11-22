"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { errorMessage, voiceService } from "@/services/voiceService";

// Keep agent source references in the saved brief and out of the editable notes.
function splitBrief(notes: string) {
  const sources: string[] = [];

  const body = notes
    .split("\n")
    .filter((line) => {
      if (/^\s*(?:[^:\n]+:\s*)?https?:\/\/\S+\s*$/i.test(line)) {
        sources.push(line);

        return false;
      }

      return true;
    })
    .join("\n")
    .trimEnd();

  return { body, sources };
}

/** Mount with the occurrence or task key so late reads cannot replace another draft. */
export function MeetingBrief({ itemKey }: { itemKey: string }) {
  const [saved, setSaved] = useState<{ notes: string; version: number } | null>(
    null
  );

  const [attempts, setAttempts] = useState<
    Awaited<ReturnType<typeof voiceService.callAttempts>>
  >([]);

  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    let active = true;
    void voiceService
      .callAttempts(itemKey)
      .then((value) => {
        if (active) {
          setAttempts(value);
        }
      })
      .catch((error_) => {
        if (active) {
          setError(errorMessage(error_));
        }
      });
    void voiceService
      .meetingBrief(itemKey)
      .then((value) => {
        if (active) {
          setSaved(value);
          setText(splitBrief(value.notes).body);
        }
      })
      .catch((error_) => {
        if (active) {
          setError(errorMessage(error_));
        }
      });

    return () => {
      active = false;
    };
  }, [itemKey]);

  async function save() {
    if (!saved || saving) {
      return;
    }

    setSaving(true);

    try {
      const next = await voiceService.saveMeetingBrief(
        itemKey,
        [text, ...splitBrief(saved.notes).sources].filter(Boolean).join("\n\n"),
        saved.version
      );

      setSaved(next);
      setText(splitBrief(next.notes).body);
      toast.success("Prep saved");
    } catch (error_) {
      setError(errorMessage(error_));
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="space-y-2">
      <Label htmlFor="meeting-brief">Meeting prep</Label>
      <Textarea
        id="meeting-brief"
        value={text}
        disabled={!saved || saving}
        onChange={(e) => setText(e.target.value)}
        className="min-h-36"
        placeholder="Purpose, part numbers, drawing and RFP references, and open questions"
      />
      {attempts.map((attempt) => (
        <p key={attempt.meeting_id} className="text-sm">
          {new Date(attempt.created_at).toLocaleDateString()} ·{" "}
          {attempt.outcomes
            .map((o) => o.kind.replaceAll("_", " "))
            .join(", ") || "Recorded call"}
        </p>
      ))}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {saved && splitBrief(saved.notes).body !== text && (
        <Button
          type="button"
          variant="outline"
          disabled={saving}
          onClick={() => void save()}
        >
          Save prep
        </Button>
      )}
    </section>
  );
}
