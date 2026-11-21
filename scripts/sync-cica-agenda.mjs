// GMK operator command. Deliver only the selected CICA calendar to Simon's CICA PC.
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const root = join(homedir(), ".config/cica-portal");
const load = async (file) => JSON.parse(await readFile(join(root,file),"utf8"));
const config = await load("calendar.json");
const calendar = config.calendars.find(c=>c.slug==="chi" && c.owner==="chi.ejimofor.00018@cicahuntek.us");
if (!calendar?.outlook_calendar_id) throw Error("Missing verified CICA calendar");
const source = await load("calendar/chi.json");
const age=Date.now()-Date.parse(source.observed);
if (!source.complete || !(age>=0 && age<900000)) throw Error("Calendar source needs refresh");
const day=date=>new Intl.DateTimeFormat("en-CA",{timeZone:"America/Phoenix",year:"numeric",month:"2-digit",day:"2-digit"}).format(date);
const today=day(Date.now());
const person=p=>({name:p?.EmailAddress?.Name??"",email:p?.EmailAddress?.Address??""});
const events=source.events.filter(e=>!e.cancelled && !e.all_day && day(new Date(e.start))===today).map(e=>({
 id:e.provider_id,title:e.title,start:e.start,end:e.end,location:e.raw.Location?.DisplayName??"",
 online:Boolean(e.raw.OnlineMeetingUrl)||/teams\.microsoft\.com\/|zoom\.us\/|meet\.google\.com\//i.test(e.raw.Body?.Content??""),
 organizer:e.raw.Organizer?person(e.raw.Organizer):null,invitees:(e.raw.Attendees??[]).filter(a=>a.Type!=="Resource").map(person)
}));
const packet={calendar:calendar.outlook_calendar_id,snapshot:{observed:source.observed,events}};
// Run on the authenticated SSH user's PC. A disabled or different calendar gets no file.
const receiver=`
$ErrorActionPreference='Stop'
$d=Join-Path $env:APPDATA 'studio.peacockery.meetily'
$p=Join-Path $d 'managed-agenda.json'
$s=Get-Content -Raw (Join-Path $d 'outlook-calendar.json') | ConvertFrom-Json
if ($s.calendar -cne '${calendar.outlook_calendar_id.replaceAll("'","''")}' -or $s.disabled) {Remove-Item $p -ErrorAction SilentlyContinue; exit 0}
$data=[Console]::In.ReadToEnd()
$tmp=$p+'.tmp'
[IO.File]::WriteAllText($tmp,$data,(New-Object Text.UTF8Encoding($false)))
if ([IO.File]::Exists($p)){[IO.File]::Replace($tmp,$p,$null)}else{[IO.File]::Move($tmp,$p)}
`;
const result=spawnSync("ssh",["-o","BatchMode=yes","-o","ConnectTimeout=5","cica","powershell -NoProfile -NonInteractive -EncodedCommand "+Buffer.from(receiver,"utf16le").toString("base64")],{input:JSON.stringify(packet),timeout:15000});
if(result.status!==0)throw Error("CICA agenda delivery failed");
