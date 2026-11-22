# Run inside the Windows desktop session with Meetily open and idle.
# Evidence stays under TEMP, including one window capture per saved meeting.
param([string]$OutputDir = (Join-Path $env:TEMP ('meetily-acceptance-' + [guid]::NewGuid())))
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = [Console]::OutputEncoding
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MeetilyWindow {
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
}
'@
$env:NODE_NO_WARNINGS='1'
$library = @'
const path=require("node:path"),{DatabaseSync}=require("node:sqlite");
const db=new DatabaseSync(path.join(process.env.APPDATA,"studio.peacockery.meetily","peacockery-meetily.sqlite"),{readOnly:true});
if(db.prepare("select 1 from meetings where status='recording'").get())throw Error("Recording active");
const rows=db.prepare("select m.id,m.title,p.state from meetings m left join meeting_publications p on p.meeting_id=m.id order by created_at desc").all();
for(const m of rows){
  const t=db.prepare("select transcript,speaker from named_transcripts where meeting_id=? order by (source='final') desc,audio_start_time,sequence limit 1").get(m.id);
  m.first=t?.transcript?.trim()||"";m.speaker=t?.speaker||"";
  m.speech=db.prepare("select transcript from named_transcripts where meeting_id=?").all(m.id).some(row=>row.transcript.replace(/\[[^\]]*\]/g,"").trim());
  m.summary=db.prepare("select 1 from summary_processes where meeting_id=? and result is not null").get(m.id)!==undefined;
}
console.log(JSON.stringify(rows));
'@ | node -
if($LASTEXITCODE -ne 0){throw 'Could not read the idle meeting library'}
$records=($library | ConvertFrom-Json)
$p=Get-Process peacockery-meetily | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1
$root=[System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
$all=[System.Windows.Automation.TreeScope]::Descendants
function Control($name,$type){
 $n=New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty),$name
 if($type){$c=New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ControlTypeProperty),$type
 $n=New-Object System.Windows.Automation.AndCondition $n,$c}
 $root.FindFirst($all,$n)
}
function AwaitControl($name,$type){
 $time=[Diagnostics.Stopwatch]::StartNew()
 do {$el=Control $name $type;if($el){return $el}; Start-Sleep -Milliseconds 120}while($time.Elapsed.TotalSeconds -lt 18)
 throw "Missing control type $type after $($time.ElapsedMilliseconds)ms in $($record.title) during $stage"
}
function Follow($name){
 $el=AwaitControl $name ([System.Windows.Automation.ControlType]::Hyperlink)
 $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Capture($file){
 $r=$root.Current.BoundingRectangle
 $b=New-Object System.Drawing.Bitmap ([int]$r.Width),([int]$r.Height)
 $g=[System.Drawing.Graphics]::FromImage($b)
 try{$dc=$g.GetHdc();try{if(![MeetilyWindow]::PrintWindow($p.MainWindowHandle,$dc,2)){throw 'Window capture failed'}}finally{$g.ReleaseHdc($dc)}
 $b.Save((Join-Path $OutputDir $file),[System.Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$b.Dispose()}
}
$button=[System.Windows.Automation.ControlType]::Button
$results=@()
foreach($record in $records){
 try{
  $stage="sidebar"; Follow $record.title
  $stage="header"; $null=AwaitControl $record.title $button
  $stage="transcript"; if($record.first){$null=AwaitControl $record.first $button}
  $stage="summary"; if(!$record.speech -and !$record.summary){$null=AwaitControl 'No speech to summarize.' $null}elseif($record.summary -or $record.state -eq 'published'){$null=AwaitControl 'Regenerate Summary' $button}
  if($record.speaker){$stage='speaker';$null=AwaitControl $record.speaker $button}
  Start-Sleep -Milliseconds 350
  Capture ($record.id+'.png')
  $results+=@{id=$record.id;title=$record.title;passed=$true}
 }catch{
  Capture ($record.id+'-failure.png')
  $results+=@{id=$record.id;title=$record.title;passed=$false;error=$_.Exception.Message}
 }
 $results|ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDir 'results.json')
}
Follow 'New meeting'
$null=AwaitControl 'Start recording' $button
Capture 'new.png'
Follow 'Calls and meeting prep'
$null=AwaitControl 'Call tasks' $null
Capture 'calls.png'
$buttons=New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ControlTypeProperty),$button
$task=$root.FindAll($all,$buttons) | Where-Object {$_.Current.Name -like 'Call *'} | Select-Object -First 1
if ($task) {
 $task.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 $null=AwaitControl 'Meeting prep' $null
 $editor=New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::AutomationIdProperty),'meeting-brief'
 $field=$root.FindFirst($all,$editor)
 if(!$field){throw 'Selected call has no prep editor'}
 Start-Sleep -Milliseconds 350
 Capture 'call-prep.png'
}
Follow 'Meetily'
$null=AwaitControl 'Start recording' $button
if(@($results|Where-Object{!$_.passed}).Count){throw 'Some saved meetings failed. See results.json.'}
