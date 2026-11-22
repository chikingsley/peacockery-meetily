import { spawnSync } from "node:child_process";
// GMK operator command. Deliver only the selected CICA calendar to Simon's CICA PC.
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

import {
  assignedCalls,
  stageReviewedTranscripts,
} from "./lib/meeting-workspace.mjs";

const root = join(homedir(), ".config/cica-portal");
const load = async (file) =>
  JSON.parse(await readFile(join(root, file), "utf8"));
const config = await load("calendar.json");
const calendar = config.calendars.find(
  (c) => c.slug === "chi" && c.owner === "chi.ejimofor.00018@cicahuntek.us"
);
if (!calendar?.outlook_calendar_id)
  throw Error("Missing verified CICA calendar");
const source = await load("calendar/chi.json");
const age = Date.now() - Date.parse(source.observed);
if (!source.complete || !(age >= 0 && age < 900000))
  throw Error("Calendar source needs refresh");
const day = (date) =>
  new Intl.DateTimeFormat("en-CA", {
    timeZone: "America/Phoenix",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(date);
const today = day(Date.now());
const through = day(Date.now() + 7 * 86_400_000);
const person = (p) => ({
  name: p?.EmailAddress?.Name ?? "",
  email: p?.EmailAddress?.Address ?? "",
});
const events = source.events
  .filter(
    (e) =>
      !e.cancelled &&
      !e.all_day &&
      day(new Date(e.end)) >= today &&
      day(new Date(e.start)) <= through
  )
  .map((e) => ({
    id: e.provider_id,
    title: e.title,
    start: e.start,
    end: e.end,
    location: e.raw.Location?.DisplayName ?? "",
    online:
      Boolean(e.raw.OnlineMeetingUrl) ||
      /teams\.microsoft\.com\/|zoom\.us\/|meet\.google\.com\//i.test(
        e.raw.Body?.Content ?? ""
      ),
    organizer: e.raw.Organizer ? person(e.raw.Organizer) : null,
    invitees: (e.raw.Attendees ?? [])
      .filter((a) => a.Type !== "Resource")
      .map(person),
  }));
const cacheDir = join(homedir(), ".cache/cica-portal");
const cacheFile = join(cacheDir, "meetily-calls-chi.json");
const stored = await readFile(cacheFile, "utf8")
  .then(JSON.parse)
  .catch(() => null);
const cached =
  stored?.calendar === calendar.outlook_calendar_id ? stored : null;
let calls = cached?.calls ?? null;
if (!cached || Date.now() - cached.observed > 300000) {
  try {
    calls = await assignedCalls("Chi Ejimofor");
    await mkdir(cacheDir, { recursive: true });
    const temporary = cacheFile + "." + process.pid;
    await writeFile(
      temporary,
      JSON.stringify({
        calendar: calendar.outlook_calendar_id,
        observed: Date.now(),
        calls,
      }),
      { mode: 0o600 }
    );
    await rename(temporary, cacheFile);
  } catch (error) {
    console.warn(error.message);
  }
}
const packet = {
  calendar: calendar.outlook_calendar_id,
  snapshot: { observed: source.observed, events, calls: calls ?? [] },
};
// Run on the authenticated SSH user's PC. A disabled or different calendar gets no file.
if (process.argv[2] === "--list") {
  console.log(
    JSON.stringify(
      [
        ...events.map((e) => ({
          key: "event:" + e.id,
          title: e.title,
          start: e.start,
        })),
        ...(calls ?? []).map((c) => ({
          key: "task:" + c.id,
          title: c.title,
          due: c.due,
          source: c.source,
        })),
      ],
      null,
      2
    )
  );
  process.exit(0);
}
if (["--read-prep", "--save-prep"].includes(process.argv[2])) {
  const [, , action, key, version] = process.argv;
  if (
    ![
      ...events.map((e) => "event:" + e.id),
      ...(calls ?? []).map((t) => "task:" + t.id),
    ].includes(key)
  )
    throw Error("Select a key from --list");
  const save = action === "--save-prep";
  const notes = save
    ? await new Promise((resolve, reject) => {
        let text = "";
        process.stdin.setEncoding("utf8");
        process.stdin.on("data", (chunk) => (text += chunk));
        process.stdin.on("end", () => resolve(text));
        process.stdin.on("error", reject);
      })
    : "";
  if (
    save &&
    (!Number.isSafeInteger(Number(version)) ||
      Number(version) < 0 ||
      version === undefined ||
      Buffer.byteLength(notes) > 131072)
  )
    throw Error(
      "Supply the current note version and up to 128 KiB of notes on stdin"
    );
  const remote = `
const fs=require('fs'),path=require('path'),base=path.join(process.env.APPDATA,'studio.peacockery.meetily');
const selected=JSON.parse(fs.readFileSync(path.join(base,'outlook-calendar.json')));
if(selected.disabled || selected.calendar!==${JSON.stringify(calendar.outlook_calendar_id)})throw Error('Selected calendar changed');
const db=new(require('node:sqlite').DatabaseSync)(path.join(base,'peacockery-meetily.sqlite'));
const key=${JSON.stringify(key)},notes=${JSON.stringify(notes)},version=${Number(version) || 0};
if(${save}){const change=version===0?db.prepare("INSERT INTO meeting_briefs(item_key,notes,version) VALUES(?,?,1) ON CONFLICT DO NOTHING").run(key,notes):db.prepare("UPDATE meeting_briefs SET notes=?,version=version+1 WHERE item_key=? AND version=?").run(notes,key,version);if(change.changes!==1)throw Error('Prep changed: read it again');}
console.log(JSON.stringify(db.prepare("SELECT notes,version FROM meeting_briefs WHERE item_key=?").get(key)??{notes:"",version:0}));`;
  const result = spawnSync(
    "ssh",
    ["-o", "BatchMode=yes", "cica", "node --no-warnings -"],
    { input: remote, encoding: "utf8" }
  );
  if (result.status !== 0)
    throw Error("Prep operation failed or the saved version changed");
  console.log(result.stdout.trim());
  process.exit(0);
}
const receiver = `
$ErrorActionPreference='Stop'
$d=Join-Path $env:APPDATA 'studio.peacockery.meetily'
$p=Join-Path $d 'managed-agenda.json'
$s=Get-Content -Raw (Join-Path $d 'outlook-calendar.json') | ConvertFrom-Json
if ($s.calendar -cne '${calendar.outlook_calendar_id.replaceAll("'", "''")}' -or $s.disabled) {Remove-Item $p -ErrorAction SilentlyContinue; exit 0}
$data=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String([Console]::In.ReadToEnd()))
$packet=$data | ConvertFrom-Json
if (${calls === null ? "$true" : "$false"} -and [IO.File]::Exists($p)){$old=Get-Content -Raw $p|ConvertFrom-Json;if($old.calendar -ceq $packet.calendar){$packet.snapshot.calls=@($old.snapshot.calls);$data=$packet|ConvertTo-Json -Depth 12 -Compress}}
$tmp=$p+'.tmp'
[IO.File]::WriteAllText($tmp,$data,(New-Object Text.UTF8Encoding($false)))
if ([IO.File]::Exists($p)){[IO.File]::Replace($tmp,$p,[System.Management.Automation.Language.NullString]::Value)}else{[IO.File]::Move($tmp,$p)}
`;
const result = spawnSync(
  "ssh",
  [
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=5",
    "cica",
    "powershell -NoProfile -NonInteractive -EncodedCommand " +
      Buffer.from(receiver, "utf16le").toString("base64"),
  ],
  {
    input: Buffer.from(JSON.stringify(packet)).toString("base64"),
    timeout: 15000,
  }
);
if (result.status !== 0) throw Error("CICA agenda delivery failed");

await stageReviewedTranscripts("cica");
