"use client";

import { useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { errorMessage, voiceService } from "@/services/voiceService";
import type { CallContact } from "@/types/voice";

const labels = {
  conversation: "Spoke with contact",
  voicemail_left: "Voicemail left",
  no_answer: "No answer",
  undetermined: "Outcome unclear",
};

type Attempt = Awaited<ReturnType<typeof voiceService.callAttempts>>[number];

export function CallAttempts({
  taskKey,
  contacts,
  disabled,
  onCall,
}: {
  taskKey: string;
  contacts: CallContact[];
  disabled: boolean;
  onCall: (contact: CallContact) => void;
}) {
  const [rows, setRows] = useState<Attempt[]>([]);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState("");
  const edits = useRef(0);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;

    async function refresh() {
      try {
        const revision = edits.current;
        const next = await voiceService.callAttempts(taskKey);

        if (active && edits.current === revision) {
          setRows(next);
          setError("");
        }
      } catch (error_) {
        if (active) {
          setError(errorMessage(error_));
        }
      } finally {
        if (active) {
          timer = setTimeout(() => void refresh(), 4000);
        }
      }
    }

    void refresh();

    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [taskKey]);

  async function save(row: Attempt, value: string) {
    try {
      await voiceService.setCallOutcome(row.meeting_id, value);
      edits.current++;
      setRows(await voiceService.callAttempts(taskKey));
      setEditing("");
      setError("");
    } catch (error_) {
      setError(errorMessage(error_));
    }
  }

  function attempt(row: Attempt) {
    const outcome = row.manual_outcome ?? row.outcomes.at(-1)?.kind;

    return (
      <div
        key={row.meeting_id}
        className="flex flex-wrap items-center gap-2 text-sm"
      >
        <time>
          {row.created_at &&
            new Date(row.created_at).toLocaleString("en-US", {
              timeZone: "America/Phoenix",
              month: "short",
              day: "numeric",
              hour: "numeric",
              minute: "2-digit",
            })}
        </time>
        <span>
          {outcome
            ? (Object.entries(labels).find(([kind]) => kind === outcome)?.[1] ??
              "Outcome unclear")
            : row.reviewed
              ? "Outcome unclear"
              : "Awaiting transcript review"}
        </span>
        {row.editable &&
          (editing === row.meeting_id ? (
            <select
              aria-label="Correct outcome"
              defaultValue=""
              onChange={(e) =>
                e.target.value === "cancel"
                  ? setEditing("")
                  : void save(row, e.target.value)
              }
            >
              <option value="" disabled>
                Select outcome
              </option>
              <option value="cancel">Cancel</option>
              <option value="automatic">Use automatic result</option>
              {Object.entries(labels).map(([k, v]) => (
                <option key={k} value={k}>
                  {v}
                </option>
              ))}
            </select>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setEditing(row.meeting_id)}
            >
              Edit outcome
            </Button>
          ))}
      </div>
    );
  }

  return (
    <section aria-label="Call contacts" className="divide-y">
      {contacts.map((c) => (
        <div key={c.number} className="space-y-1 py-3">
          <div className="flex items-center justify-between gap-4">
            <div>
              <strong>{c.name}</strong>
              <div className="text-sm tabular-nums">
                {c.number.replace(/^\+1(\d{3})(\d{3})(\d{4})$/, "($1) $2-$3")}
              </div>
            </div>
            <Button disabled={disabled} onClick={() => onCall(c)}>
              Call {c.name}
            </Button>
          </div>
          {rows.filter((r) => r.number === c.number).map(attempt)}
        </div>
      ))}
      {rows
        .filter((r) => !contacts.some((c) => c.number === r.number))
        .map((r) => (
          <div key={r.meeting_id}>
            {r.name ?? "Previous attempt"} {r.number}
            {attempt(r)}
          </div>
        ))}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
