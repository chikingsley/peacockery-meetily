import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { errorMessage, voiceService } from "@/services/voiceService";

export function CallTaskActions({ id }: { id: string }) {
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [date, setDate] = useState("");
  useEffect(() => {
    let alive = true;

    async function poll() {
      try {
        const value = await voiceService.callChangeStatus(id);

        if (alive && value) {
          setStatus(value);
        }
      } catch (error) {
        if (alive) {
          setStatus(errorMessage(error));
        }
      }
    }

    void poll();
    const timer = setInterval(() => void poll(), 4000);

    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [id]);

  async function save(due: string | null) {
    setBusy(true);

    try {
      await voiceService.changeCallTask(id, due);
      setStatus("Queued for Trello");
      setEditing(false);
    } catch (error) {
      setStatus(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  const locked = busy || status === "Queued for Trello";

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={locked}
          onClick={() => void save(null)}
        >
          Mark complete
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={locked}
          onClick={() => setEditing(!editing)}
        >
          {editing ? "Cancel" : "Reschedule"}
        </Button>
      </div>
      {editing && (
        <div className="space-y-1">
          <label htmlFor="call-due" className="text-sm">
            New date and time (Phoenix)
          </label>
          <div className="flex gap-2">
            <Input
              id="call-due"
              type="datetime-local"
              value={date}
              onChange={(e) => setDate(e.target.value)}
            />
            <Button
              size="sm"
              disabled={locked || !date}
              onClick={() => void save(new Date(`${date}-07:00`).toISOString())}
            >
              Save
            </Button>
          </div>
        </div>
      )}
      {status && (
        <p role="status" className="text-sm text-muted-foreground">
          {status}
        </p>
      )}
    </div>
  );
}
