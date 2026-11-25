// The managed agenda transports requests; Trello owns completion and due dates.
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { assignedCalls } from "./meeting-workspace.mjs";

function exchange(calendar, receipts) {
  const fs = require("node:fs"), path = require("node:path");
  const base = path.join(process.env.APPDATA, "studio.peacockery.meetily");
  if (JSON.parse(fs.readFileSync(path.join(base, "outlook-calendar.json"))).calendar !== calendar) return [];
  const dir = path.join(base, "call-changes");
  if (!fs.existsSync(dir)) return [];
  const pending = [];
  for (const name of fs.readdirSync(dir).filter(n => /^[a-f0-9-]{36}\.json$/i.test(n))) {
    const file = path.join(dir, name), change = JSON.parse(fs.readFileSync(file));
    if (change.calendar !== calendar || name !== change.id + ".json") continue;
    const receipt = receipts.find(r => r.request === change.request);
    if (receipt) {
      fs.writeFileSync(file + ".result.tmp", JSON.stringify(receipt));
      fs.renameSync(file + ".result.tmp", file + ".result");
      fs.unlinkSync(file);
    } else pending.push(change);
  }
  return pending;
}

function pc(calendar, receipts) {
  const result = spawnSync("ssh", ["-o","BatchMode=yes","-o","ConnectTimeout=5","cica","node --no-warnings -"], {
    input: `console.log(JSON.stringify((${exchange.toString()})(${JSON.stringify(calendar)},${JSON.stringify(receipts)})))`, encoding:"utf8",timeout:15000,
  });
  if (result.status !== 0) throw Error("Call edit exchange unavailable");
  return JSON.parse(result.stdout);
}

export function changeDecision(task, edit) {
  if (!task?.editable || task.title !== edit.title) return "Task changed. Review the current task.";
  if (edit.due === null && task.state === "complete") return "Saved";
  if (edit.due !== null && task.state === "incomplete" && Date.parse(task.due) === Date.parse(edit.due)) return "Saved";
  if (task.state !== "incomplete" || task.due !== edit.before) return "Task changed. Review the current task.";
  if (edit.due !== null && !(Date.parse(edit.due) > Date.now())) return "Choose a future due time.";
  return "write";
}

export async function processCallChanges(calendar) {
  const edits = pc(calendar, []);
  if (!edits.length) return false;
  const tasks = await assignedCalls("Chi Ejimofor", true);
  const receipts = [];
  for (const edit of edits) {
    const task = tasks.find(t => t.id === edit.id);
    let status = changeDecision(task, edit);
    if (status === "write") {
      const result = spawnSync(join(homedir(),".local/bin/trello"), ["api","PUT",`/cards/${task.cardId}/checkItem/${task.itemId}`,"-q",edit.due === null ? "state=complete" : `due=${edit.due}`], {encoding:"utf8",timeout:12000});
      if (result.status !== 0) continue;
      const saved = JSON.parse(result.stdout);
      if (saved.id !== task.itemId || (edit.due === null ? saved.state !== "complete" : Date.parse(saved.due) !== Date.parse(edit.due))) continue;
      status = "Saved";
    }
    receipts.push({request:edit.request, status});
  }
  pc(calendar, receipts);
  return true;
}
