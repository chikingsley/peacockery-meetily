"use client";

import { useEffect, useState } from "react";
import type { ReactNode } from "react";

import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { modelChoices, modelOptionText } from "@/lib/voice-models";
import type { ModelOption } from "@/types/voice";

export function SettingsSection({
  title,
  icon,
  description,
  children,
}: {
  title: string;
  icon?: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="space-y-4 rounded-lg border bg-background p-4">
      <div>
        <h3 className="flex items-center gap-2 text-sm font-semibold text-foreground">
          {icon}
          {title}
        </h3>
        {description && (
          <p className="mt-1 text-sm text-muted-foreground">{description}</p>
        )}
      </div>
      {children}
    </section>
  );
}

export function ToggleRow({
  id,
  label,
  description,
  checked,
  onChange,
  disabled = false,
}: {
  id: string;
  label: string;
  description?: ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-6">
      <div className="space-y-1">
        <Label htmlFor={id}>{label}</Label>
        {description && (
          <p className="text-xs text-muted-foreground">{description}</p>
        )}
      </div>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
        className="mt-0.5"
      />
    </div>
  );
}

/** Model select from a `voice_list_models` list: label, note under it, unavailable models disabled. */
export function ModelSelect({
  id,
  label,
  value,
  options,
  onChange,
  disabled = false,
}: {
  id: string;
  label: string;
  value: string;
  options: ModelOption[];
  onChange: (id: string) => void;
  disabled?: boolean;
}) {
  const choices = modelChoices(options, value);
  const note = choices.find((o) => o.id === value)?.note;

  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Select value={value} onValueChange={onChange} disabled={disabled}>
        <SelectTrigger id={id} className="w-full">
          <SelectValue placeholder={label} />
        </SelectTrigger>
        <SelectContent>
          {choices.map((o) => (
            <SelectItem
              key={o.id}
              value={o.id}
              disabled={!o.available}
              description={o.note || undefined}
            >
              {modelOptionText(o)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {note && <p className="text-xs text-muted-foreground">{note}</p>}
    </div>
  );
}

/**
 * A text setting edited locally and committed on blur or Enter. Follows the saved value when it
 * changes elsewhere (e.g. `voice-settings-updated`).
 */
export function useCommitDraft(value: string, commit: (next: string) => void) {
  const [draft, setDraft] = useState(value);
  useEffect(() => {
    setDraft(value);
  }, [value]);

  const flush = () => {
    if (draft !== value) {
      commit(draft);
    }
  };

  const commitNow = (next: string) => {
    setDraft(next);

    if (next !== value) {
      commit(next);
    }
  };

  return { commitNow, draft, flush, setDraft };
}
