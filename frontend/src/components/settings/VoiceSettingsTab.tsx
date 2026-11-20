"use client";

import {
  AudioLines,
  CheckCircle2,
  FileText,
  KeyRound,
  Languages,
  Loader2,
  Sparkles,
  XCircle,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useConfig } from "@/contexts/ConfigContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import {
  AUTO_FINAL_CHOICES,
  LANGUAGES,
  formatKeyterms,
  modelLabel,
  parseKeyterms,
} from "@/lib/voice-models";
import { voiceService, errorMessage } from "@/services/voiceService";
import type { ConnectionTestResult, VoiceSettings } from "@/types/voice";

import { ActivationForm } from "./ActivationForm";
import {
  ModelSelect,
  SettingsSection,
  ToggleRow,
  useCommitDraft,
} from "./SettingsParts";

/** Settings → Peacockery Voice: key, connection, live / final / summary models and transcript options. */
export function VoiceSettingsTab() {
  const { voiceSettings, saveVoiceSettings, voiceModels } = useConfig();
  const { isRecording } = useRecordingState();

  if (!voiceSettings) {
    return (
      <div className="space-y-6">
        <ApiKeySection />
        <div className="h-40 animate-pulse rounded-lg bg-muted/40" />
      </div>
    );
  }

  const update = (patch: Partial<VoiceSettings>) => {
    saveVoiceSettings({ ...voiceSettings, ...patch }).catch((error) =>
      toast.error("Failed to save settings", {
        description: errorMessage(error),
      })
    );
  };

  return (
    <div className="space-y-6">
      <ApiKeySection />

      <SettingsSection
        title="Live transcription"
        icon={<AudioLines className="h-5 w-5" />}
        description={
          isRecording
            ? "These apply to the next recording; the current one keeps its live settings."
            : "Transcribes while you record and keeps the transcript when you stop."
        }
      >
        <ToggleRow
          id="live-enabled"
          label="Live transcript while recording"
          checked={voiceSettings.live_enabled}
          onChange={(live_enabled) => update({ live_enabled })}
        />
        <ModelSelect
          id="live-model"
          label="Live model"
          value={voiceSettings.live_model}
          options={voiceModels?.live ?? []}
          disabled={!voiceSettings.live_enabled}
          onChange={(live_model) => update({ live_model })}
        />
        <p className="text-sm text-muted-foreground">
          Choose In person, Online meeting or Phone call on the recording
          screen. Speaker separation applies to every captured channel when the
          live model supports it.
        </p>
      </SettingsSection>

      <SettingsSection
        title="Final transcript"
        icon={<FileText className="h-5 w-5" />}
        description="Transcribes the complete saved audio with speaker labels when the live transcript cannot serve as the result."
      >
        <ModelSelect
          id="final-model"
          label="Final model"
          value={voiceSettings.final_model}
          options={voiceModels?.final ?? []}
          onChange={(final_model) => update({ final_model })}
        />
        <ToggleRow
          id="final-diarize"
          label="Speaker labels"
          description="Uses numbered speakers. A microphone may contain several people; names can be verified afterward."
          checked={voiceSettings.final_diarize}
          onChange={(final_diarize) => update({ final_diarize })}
        />
        <div className="space-y-1.5">
          <Label htmlFor="auto-final">Run after stop</Label>
          <Select
            value={voiceSettings.auto_final}
            onValueChange={(value) => {
              const choice = AUTO_FINAL_CHOICES.find((c) => c.value === value);

              if (choice) {
                update({ auto_final: choice.value });
              }
            }}
          >
            <SelectTrigger id="auto-final" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {AUTO_FINAL_CHOICES.map((c) => (
                <SelectItem key={c.value} value={c.value}>
                  {c.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            Recommended: a live transcript that ran completely on{" "}
            {modelLabel(voiceModels?.final, voiceSettings.final_model)} is kept
            as the result.
          </p>
        </div>
      </SettingsSection>

      <LanguageSection settings={voiceSettings} update={update} />

      <SettingsSection title="Summary" icon={<Sparkles className="h-5 w-5" />}>
        <ModelSelect
          id="summary-model"
          label="Summary model"
          value={voiceSettings.summary_model}
          options={voiceModels?.summary ?? []}
          onChange={(summary_model) => update({ summary_model })}
        />
        <ToggleRow
          id="auto-summary"
          label="Summarize automatically"
          description="Runs after recording stops, or after the final transcript finishes when one runs."
          checked={voiceSettings.auto_summary}
          onChange={(auto_summary) => update({ auto_summary })}
        />
      </SettingsSection>
    </div>
  );
}

function LanguageSection({
  settings,
  update,
}: {
  settings: VoiceSettings;
  update: (patch: Partial<VoiceSettings>) => void;
}) {
  const keyterms = useCommitDraft(formatKeyterms(settings.keyterms), (text) =>
    update({ keyterms: parseKeyterms(text) })
  );

  const corrections = useCommitDraft(
    settings.transcript_corrections
      .map((rule) => `${rule.heard} => ${rule.replacement}`)
      .join("\n"),
    (text) =>
      update({
        transcript_corrections: text.split("\n").flatMap((line) => {
          const separator = line.indexOf("=>");

          if (separator < 1) {
            return [];
          }

          const heard = line.slice(0, separator).trim();
          const replacement = line.slice(separator + 2).trim();

          return heard && replacement ? [{ heard, replacement }] : [];
        }),
      })
  );

  const meLabel = useCommitDraft(settings.me_label, (me_label) =>
    update({ me_label: me_label.trim() || "Me" })
  );

  const themLabel = useCommitDraft(settings.them_label, (them_label) =>
    update({ them_label: them_label.trim() || "Them" })
  );

  const knownLanguage = LANGUAGES.some((l) => l.code === settings.language);

  return (
    <SettingsSection
      title="Language and names"
      icon={<Languages className="h-5 w-5" />}
    >
      <div className="space-y-1.5">
        <Label htmlFor="language">Language</Label>
        <Select
          value={settings.language}
          onValueChange={(language) => update({ language })}
        >
          <SelectTrigger id="language" className="w-full md:w-72">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {!knownLanguage && (
              <SelectItem value={settings.language}>
                {settings.language}
              </SelectItem>
            )}
            {LANGUAGES.map((l) => (
              <SelectItem key={l.code} value={l.code}>
                {l.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          Used for live and final transcripts. A fixed language is more accurate
          than detection.
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="keyterms">Vocabulary hints</Label>
        <Textarea
          id="keyterms"
          rows={4}
          placeholder={"Cica-Huntek\nAltaflo\nNetask"}
          value={keyterms.draft}
          onChange={(e) => keyterms.setDraft(e.target.value)}
          onBlur={keyterms.flush}
        />
        <p className="text-xs text-muted-foreground">
          Names, companies and jargon, one per line or comma-separated. Sent to
          models that accept hints.
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="transcript-corrections">Recognition corrections</Label>
        <Textarea
          id="transcript-corrections"
          rows={3}
          value={corrections.draft}
          onChange={(e) => corrections.setDraft(e.target.value)}
          onBlur={corrections.flush}
          placeholder="Sika Huntech => Cica-Huntek"
        />
        <p className="text-xs text-muted-foreground">
          One heard phrase =&gt; correct spelling per line. Applies to future
          live and reprocessed transcripts; original provider artifacts stay
          preserved.
        </p>
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="me-label">Your label</Label>
          <Input
            id="me-label"
            value={meLabel.draft}
            onChange={(e) => meLabel.setDraft(e.target.value)}
            onBlur={meLabel.flush}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                meLabel.flush();
              }
            }}
          />
          <p className="text-xs text-muted-foreground">
            Lines from your microphone.
          </p>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="them-label">Other side label</Label>
          <Input
            id="them-label"
            value={themLabel.draft}
            onChange={(e) => themLabel.setDraft(e.target.value)}
            onBlur={themLabel.flush}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                themLabel.flush();
              }
            }}
          />
          <p className="text-xs text-muted-foreground">
            Lines from the call audio; several people become &quot;
            {settings.them_label} 1&quot;, &quot;{settings.them_label} 2&quot;.
          </p>
        </div>
      </div>
    </SettingsSection>
  );
}

/**
 * Access: activation code entry, key status (Rust stores the key in Windows Credential Manager;
 * the page only sees the masked form) and connection test.
 */
function ApiKeySection() {
  const { voiceModels, refreshVoiceModels, keyStatus } = useConfig();

  const [isTesting, setIsTesting] = useState(false);

  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(
    null
  );

  const testConnection = async () => {
    setIsTesting(true);

    try {
      setTestResult(await voiceService.testConnection());
      await refreshVoiceModels();
    } catch (error) {
      setTestResult({
        environment: null,
        error: errorMessage(error),
        latency_ms: 0,
        ok: false,
      });
    } finally {
      setIsTesting(false);
    }
  };

  return (
    <SettingsSection title="Access" icon={<KeyRound className="h-5 w-5" />}>
      <p className="text-sm text-muted-foreground">
        Enter the activation code you received.
      </p>
      <ActivationForm />
      <div className="flex items-center gap-2 text-sm">
        <span className="font-medium text-foreground">Access</span>
        {keyStatus?.configured ? (
          <span className="inline-flex items-center gap-1 rounded-full bg-success/10 px-2 py-0.5 text-xs font-medium text-success">
            <CheckCircle2 className="h-3 w-3" /> Active on this PC ·{" "}
            <span className="font-mono">{keyStatus.masked}</span>
          </span>
        ) : (
          <span className="rounded-full bg-warning/10 px-2 py-0.5 text-xs font-medium text-warning">
            Not activated
          </span>
        )}
      </div>
      <p className="text-xs text-muted-foreground">
        Kept in Windows Credential Manager, never in the app&apos;s files.
      </p>

      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="outline"
          size="sm"
          onClick={() => void testConnection()}
          disabled={isTesting || !keyStatus?.configured}
        >
          {isTesting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          Test connection
        </Button>
        {testResult &&
          (testResult.ok ? (
            <span className="flex items-center gap-1 text-sm text-success">
              <CheckCircle2 className="h-4 w-4" />
              Connected
              {testResult.environment
                ? ` (${testResult.environment})`
                : ""} ·{" "}
              {testResult.latency_ms} ms
            </span>
          ) : (
            <span className="flex items-center gap-1 text-sm text-destructive">
              <XCircle className="h-4 w-4" />
              {testResult.error ?? "Connection failed"}
            </span>
          ))}
      </div>
      {keyStatus?.configured && voiceModels?.error && (
        <p className="text-sm text-warning" role="status">
          Could not read the server&apos;s model list ({voiceModels.error}). The
          pickers below show the built-in list.
        </p>
      )}
    </SettingsSection>
  );
}
