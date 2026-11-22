// GMK owns the CRM mirror and the portal's staged recording folders.
import { spawnSync } from "node:child_process";
import { readFile, readdir, rename, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

export async function assignedCalls(memberName) {
  const key = (
    await readFile(join(homedir(), "github/cica/crm/.agent-api-key"), "utf8")
  ).trim();
  async function records(collection, filters = {}) {
    const rows = [];
    let cursor;
    do {
      const url = new URL("/rest/" + collection, "http://127.0.0.1:3020");
      url.search = new URLSearchParams({ limit: "60", depth: "0", ...filters });
      if (cursor) url.searchParams.set("starting_after", cursor);
      const response = await fetch(url, {
        headers: { authorization: "Bearer " + key },
        signal: AbortSignal.timeout(7000),
      });
      if (!response.ok)
        throw Error("CRM call list returned " + response.status);
      const page = await response.json();
      rows.push(...page.data[collection]);
      cursor = page.pageInfo?.hasNextPage ? page.pageInfo.endCursor : null;
    } while (cursor);
    return rows;
  }
  const members = (await records("workspaceMembers")).filter(
    (m) => [m.name.firstName, m.name.lastName].join(" ") === memberName
  );
  if (members.length !== 1)
    throw Error("Expected a unique assigned workspace member");
  const tasks = await records("tasks");
  return tasks
    .filter(
      (t) =>
        t.assigneeId === members[0].id &&
        t.taskType === "CALL" &&
        t.status !== "DONE" &&
        !t.deletedAt
    )
    .sort((a, b) => (a.dueAt ?? "").localeCompare(b.dueAt ?? ""))
    .map((t) => {
      const phone = t.phone ?? {};
      const digits = (phone.primaryPhoneNumber ?? "").replace(/\D/g, "");
      return {
        id: t.id,
        title: t.title,
        company: t.trelloCard?.primaryLinkLabel ?? "",
        source: t.trelloCard?.primaryLinkUrl ?? "",
        number: digits ? (phone.primaryPhoneCallingCode ?? "") + digits : "",
        due: t.dueAt ?? "",
      };
    });
}

export async function stageReviewedTranscripts(host) {
  // Only the recorder's previously published rows are eligible. Read through the
  // same view used by transcript export. A user rename also changes this output.
  const read = `
const fs=require("node:fs"),path=require("node:path"),{DatabaseSync}=require("node:sqlite");
const db=new DatabaseSync(path.join(process.env.APPDATA,"studio.peacockery.meetily","peacockery-meetily.sqlite"),{readOnly:true});
if (!db.prepare("SELECT 1 FROM sqlite_master WHERE name='named_transcripts'").get())process.exit(0);
const records=db.prepare("SELECT p.meeting_id,p.upload_id FROM meeting_publications p JOIN meeting_reviews r ON r.meeting_id=p.meeting_id WHERE p.state='published' AND r.revision LIKE 'end:%'").all();
const turns=db.prepare("SELECT speaker,audio_start_time AS at,transcript AS text FROM named_transcripts WHERE meeting_id=? AND source=CASE WHEN EXISTS(SELECT 1 FROM transcripts WHERE meeting_id=? AND source='final') THEN 'final' ELSE 'live' END ORDER BY audio_start_time,sequence");
console.log(JSON.stringify(records.map(r=>({id:r.upload_id,turns:turns.all(r.meeting_id,r.meeting_id)}))));`;
  const result = spawnSync(
    "ssh",
    [
      "-o",
      "BatchMode=yes",
      "-o",
      "ConnectTimeout=5",
      host,
      "node --no-warnings -",
    ],
    {
      input: read,
      encoding: "utf8",
      timeout: 15000,
      maxBuffer: 32 * 1024 * 1024,
    }
  );
  if (result.status !== 0)
    throw Error("Desktop transcript readback unavailable");
  for (const row of JSON.parse(result.stdout.trim() || "[]")) {
    if (!/^[a-z0-9-]+$/.test(row.id) || !row.turns.length) continue;
    const folder = join(
      homedir(),
      "github/cica/reference/company-context-live/meetings",
      row.id
    );
    const names = await readdir(folder).catch(() => []);
    const name = "Meetily Transcript.md";
    if (
      !names.includes("source.json") ||
      names.some((n) => n !== name && /(?:transcript|moss).*\.md$/i.test(n))
    )
      continue;
    const source = JSON.parse(
      await readFile(join(folder, "source.json"), "utf8")
    );
    if (
      source.partial_capture ||
      source.transcript_end_seconds ||
      source.capture_note ||
      names.includes("held-from-company-folder")
    )
      continue;
    const content =
      row.turns
        .map((t) => {
          const s = Math.max(0, Math.floor(t.at ?? 0));
          const time = [
            Math.floor(s / 3600),
            Math.floor((s % 3600) / 60),
            s % 60,
          ]
            .map((n) => String(n).padStart(2, "0"))
            .join(":");
          return (
            time +
            " " +
            (t.speaker || "Speaker").replace(/[:\r\n]/g, " ") +
            ": " +
            t.text.replace(/[\r\n]+/g, " ")
          );
        })
        .join("\n") + "\n";
    const file = join(folder, name);
    if ((await readFile(file, "utf8").catch(() => null)) !== content) {
      const temporary = file + "." + process.pid + ".tmp";
      await writeFile(temporary, content, { mode: 0o600 });
      await rename(temporary, file);
    }
  }
}

// Executed by the desktop's Node runtime, against the same data used by Meetily.
function editRecording(action, id, expected, patch) {
  const path = require("node:path");
  const { createHash } = require("node:crypto");
  const { DatabaseSync } = require("node:sqlite");
  const dir = path.join(process.env.APPDATA, "studio.peacockery.meetily");
  const db = new DatabaseSync(path.join(dir, "peacockery-meetily.sqlite"), {
    readOnly: action !== "--edit-recording",
  });
  db.exec("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000");
  if (action === "--list-recordings") {
    return db
      .prepare(
        "SELECT id,title,created_at FROM meetings ORDER BY created_at DESC"
      )
      .all();
  }
  function read() {
    const meeting = db
      .prepare("SELECT id,title,status FROM meetings WHERE id=?")
      .get(id);
    if (!meeting) throw Error("Unknown recording");
    const summary = db
      .prepare(
        "SELECT status,result,metadata FROM summary_processes WHERE meeting_id=?"
      )
      .get(id);
    const speakers = db
      .prepare(
        "SELECT t.source,t.speaker AS label,n.name,count(*) AS turns FROM transcripts t LEFT JOIN speaker_names n ON (n.meeting_id,n.source,n.label)=(t.meeting_id,t.source,t.speaker) WHERE t.meeting_id=? AND t.speaker IS NOT NULL GROUP BY t.source,t.speaker ORDER BY t.source,t.speaker"
      )
      .all(id);
    const version = createHash("sha256")
      .update(JSON.stringify([meeting, summary, speakers]))
      .digest("hex");
    return {
      ...meeting,
      version,
      markdown: JSON.parse(summary?.result ?? "null")?.markdown ?? "",
      speakers,
      summaryStatus: summary?.status,
    };
  }
  db.exec(action === "--edit-recording" ? "BEGIN IMMEDIATE" : "BEGIN");
  const record = read();
  if (action === "--edit-recording") {
    if (record.version !== expected || record.status === "recording")
      throw Error("Read the recording again before editing");
    if (
      !patch ||
      !Object.keys(patch).length ||
      Object.keys(patch).some(
        (k) => !["title", "markdown", "speaker"].includes(k)
      )
    )
      throw Error("Expected title, markdown or speaker");
    const now = new Date().toISOString();
    if ("title" in patch) {
      if (typeof patch.title !== "string" || !patch.title.trim())
        throw Error("Title required");
      db.prepare("UPDATE meetings SET title=?,updated_at=? WHERE id=?").run(
        patch.title.trim(),
        now,
        id
      );
    }
    if ("markdown" in patch) {
      if (
        typeof patch.markdown !== "string" ||
        !patch.markdown.trim() ||
        record.summaryStatus?.toLowerCase() === "pending"
      )
        throw Error("Enter notes after generation finishes");
      db.prepare(
        "INSERT INTO summary_processes(meeting_id,status,result,created_at,updated_at) VALUES(?,'completed',?,?,?) ON CONFLICT(meeting_id) DO UPDATE SET result=excluded.result,status='completed',updated_at=excluded.updated_at,metadata=json_remove(metadata,'$.publication_id')"
      ).run(id, JSON.stringify({ markdown: patch.markdown }), now, now);
    }
    if ("speaker" in patch) {
      const { source, label, name } = patch.speaker;
      if (
        !record.speakers.some(
          (s) => s.source === source && s.label === label
        ) ||
        typeof name !== "string" ||
        !name.trim()
      )
        throw Error("Select a source and label from the recording");
      db.prepare(
        "INSERT INTO speaker_names(meeting_id,source,label,name,evidence,manual) VALUES(?,?,?,?,'Agent edit',1) ON CONFLICT(meeting_id,source,label) DO UPDATE SET name=excluded.name,evidence=excluded.evidence,manual=1"
      ).run(id, source, label, name.trim());
    }
    db.prepare("UPDATE meetings SET updated_at=? WHERE id=?").run(now, id);
  }
  const saved = action === "--edit-recording" ? read() : record;
  db.exec("COMMIT");
  return saved;
}

export async function recordingCommand(host, action, id, version) {
  const chunks = [];
  if (action === "--edit-recording")
    for await (const chunk of process.stdin) chunks.push(chunk);
  const patch = chunks.length
    ? JSON.parse(Buffer.concat(chunks).toString("utf8"))
    : null;
  const input = `console.log(JSON.stringify((${editRecording.toString()})(...${JSON.stringify([action, id, version, patch])})))`;
  const result = spawnSync(
    "ssh",
    ["-o", "BatchMode=yes", host, "node --no-warnings -"],
    { input, encoding: "utf8", maxBuffer: 8 * 1024 * 1024 }
  );
  if (result.status !== 0)
    throw Error(
      result.stderr.match(/^Error: (.+)$/m)?.[1] ?? "Recording command failed"
    );
  console.log(result.stdout.trim());
}
