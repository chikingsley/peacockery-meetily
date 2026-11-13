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
import { useRef, useState } from "react";
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
  DEFAULT_BASE_URL,
  LANGUAGES,
  formatKeyterms,
  liveSpeakerLabelsHelp,
  parseKeyterms,
  supportsLiveSpeakerLabels,
} from "@/lib/voice-models";
import { voiceService, errorMessage } from "@/services/voiceService";
import type {
  ConnectionTestResult,
  LiveMode,
  VoiceSettings,
} from "@/types/voice";

import {
  ModelSelect,
  SettingsSection,
  ToggleRow,
  useCommitDraft,
} from "./SettingsParts";

const LIVE_MODES: {
  value: LiveMode;
  title: string;
  badge?: string;
  body: string;
}[] = [
  {
    badge: "Recommended for headset calls",
    body: "Your mic and the call audio are transcribed as two streams, so every line is labelled with who said it.",
    title: "Split “Me / Them”",
    value: "split",
  },
  {
    body: "One stream of the mic and call audio together. Use it for a room or speakerphone, where the mic hears everyone.",
    title: "Mixed",
    value: "mixed",
  },
];

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

  const speakerLabelsSupported = supportsLiveSpeakerLabels(
    voiceSettings.live_model
  );

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
          onChange={(live_model) =>
            update({
              live_diarize: supportsLiveSpeakerLabels(live_model)
                ? voiceSettings.live_diarize
                : false,
              live_model,
            })
          }
        />
        <div className="space-y-2">
          <p className="text-sm font-medium">Mode</p>
          <div
            role="radiogroup"
            aria-label="Live mode"
            className="grid gap-3 md:grid-cols-2"
          >
            {LIVE_MODES.map((mode) => {
              const selected = voiceSettings.live_mode === mode.value;

              return (
                <button
                  type="button"
                  key={mode.value}
                  role="radio"
                  aria-checked={selected}
                  disabled={!voiceSettings.live_enabled}
                  onClick={() => update({ live_mode: mode.value })}
                  className={`rounded-lg border p-3 text-left transition-colors disabled:opacity-50 ${
                    selected
                      ? "border-info/30 bg-info/10 ring-1 ring-info"
                      : "border-border hover:border-border hover:bg-muted/40"
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span
                      className={`h-3.5 w-3.5 rounded-full border ${selected ? "border-4 border-info/30" : "border-border"}`}
                    />
                    <span className="text-sm font-medium text-foreground">
                      {mode.title}
                    </span>
                  </div>
                  {mode.badge && (
                    <span className="mt-1 inline-block text-xs font-medium text-info">
                      {mode.badge}
                    </span>
                  )}
                  <p className="mt-1 text-xs text-muted-foreground">
                    {mode.body}
                  </p>
                </button>
              );
            })}
          </div>
        </div>
        <ToggleRow
          id="live-diarize"
          label="Live speaker labels"
          description={
            speakerLabelsSupported
              ? liveSpeakerLabelsHelp(
                  voiceSettings.live_mode,
                  voiceSettings.them_label
                )
              : "Scribe v3 Live and Voxtral Mini Realtime 3 support live speaker labels."
          }
          checked={speakerLabelsSupported && voiceSettings.live_diarize}
          disabled={!voiceSettings.live_enabled || !speakerLabelsSupported}
          onChange={(live_diarize) => update({ live_diarize })}
        />
      </SettingsSection>

      <SettingsSection
        title="Optional final transcript"
        icon={<FileText className="h-5 w-5" />}
        description="Optionally reprocess the saved audio. Summaries use this version when available, or your saved live transcript."
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
          description={`Speakers are named "${voiceSettings.me_label}" and "${voiceSettings.them_label}" from which side of the call they were heard on.`}
          checked={voiceSettings.final_diarize}
          onChange={(final_diarize) => update({ final_diarize })}
        />
        <ToggleRow
          id="auto-final"
          label="Run automatically after stop"
          description="Off: run it yourself from the meeting page."
          checked={voiceSettings.auto_final}
          onChange={(auto_final) => update({ auto_final })}
        />
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
          description="Runs after recording stops, or after the optional final pass finishes."
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
 * API key (Rust stores it in Windows Credential Manager; the page only sees the masked form),
 * base URL and connection test. The key input is uncontrolled and emptied after submit, so the
 * key never sits in React state.
 */
function ApiKeySection() {
  const {
    voiceSettings,
    saveVoiceSettings,
    voiceModels,
    refreshVoiceModels,
    keyStatus,
    setKeyStatus,
  } = useConfig();

  const keyInputRef = useRef<HTMLInputElement>(null);
  const [hasKeyInput, setHasKeyInput] = useState(false);
  const [isSavingKey, setIsSavingKey] = useState(false);
  const [isTesting, setIsTesting] = useState(false);

  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(
    null
  );

  const baseUrl = useCommitDraft(voiceSettings?.base_url ?? "", (base_url) => {
    if (!voiceSettings) {
      return;
    }

    const next = base_url.trim().replace(/\/+$/, "") || DEFAULT_BASE_URL;
    setTestResult(null);
    saveVoiceSettings({ ...voiceSettings, base_url: next })
      .then(() => refreshVoiceModels())
      .catch((error) =>
        toast.error("Failed to save the base URL", {
          description: errorMessage(error),
        })
      );
  });

  const saveKey = async () => {
    const input = keyInputRef.current;
    const key = input?.value.trim() ?? "";

    if (!input || !key) {
      return;
    }

    setIsSavingKey(true);

    try {
      setKeyStatus(await voiceService.setApiKey(key));
      input.value = "";
      setHasKeyInput(false);
      setTestResult(null);
      await refreshVoiceModels();
      toast.success("API key saved");
    } catch (error) {
      toast.error("Failed to save API key", {
        description: errorMessage(error),
      });
    } finally {
      setIsSavingKey(false);
    }
  };

  const clearKey = async () => {
    try {
      setKeyStatus(await voiceService.clearApiKey());
      setTestResult(null);
      await refreshVoiceModels();
      toast.success("API key removed");
    } catch (error) {
      toast.error("Failed to clear API key", {
        description: errorMessage(error),
      });
    }
  };

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
    <SettingsSection title="Connection" icon={<KeyRound className="h-5 w-5" />}>
      <div className="space-y-2">
        <div className="flex items-center gap-2 text-sm">
          <span className="font-medium text-foreground">API key</span>
          {keyStatus?.configured ? (
            <span className="inline-flex items-center gap-1 rounded-full bg-success/10 px-2 py-0.5 text-xs font-medium text-success">
              <CheckCircle2 className="h-3 w-3" /> Stored ·{" "}
              <span className="font-mono">{keyStatus.masked}</span>
            </span>
          ) : (
            <span className="rounded-full bg-warning/10 px-2 py-0.5 text-xs font-medium text-warning">
              Not set
            </span>
          )}
        </div>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void saveKey();
          }}
        >
          <Input
            ref={keyInputRef}
            type="password"
            autoComplete="off"
            spellCheck={false}
            aria-label="Peacockery Voice API key"
            placeholder={
              keyStatus?.configured
                ? "Paste a new key to replace the stored one"
                : "pv_lab_…"
            }
            onChange={(e) => setHasKeyInput(e.target.value.trim().length > 0)}
          />
          <Button type="submit" disabled={!hasKeyInput || isSavingKey}>
            {isSavingKey ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : keyStatus?.configured ? (
              "Replace"
            ) : (
              "Save"
            )}
          </Button>
          {keyStatus?.configured && (
            <Button
              type="button"
              variant="outline"
              onClick={() => void clearKey()}
            >
              Clear
            </Button>
          )}
        </form>
        <p className="text-xs text-muted-foreground">
          Kept in Windows Credential Manager, never in the app&apos;s files.
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="base-url">Server</Label>
        <div className="flex gap-2">
          <Input
            id="base-url"
            spellCheck={false}
            value={baseUrl.draft}
            onChange={(e) => baseUrl.setDraft(e.target.value)}
            onBlur={baseUrl.flush}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                baseUrl.flush();
              }
            }}
          />
          {voiceSettings && voiceSettings.base_url !== DEFAULT_BASE_URL && (
            <Button
              type="button"
              variant="outline"
              onClick={() => baseUrl.commitNow(DEFAULT_BASE_URL)}
            >
              Use lab
            </Button>
          )}
        </div>
        <p className="text-xs text-muted-foreground">
          Lab keys (pv_lab_…) only work on {DEFAULT_BASE_URL}.
        </p>
      </div>

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
