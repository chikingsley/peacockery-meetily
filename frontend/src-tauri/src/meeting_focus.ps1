# Embedded in Meetily. Run only from its native recorder lifecycle in the user's session.
$ErrorActionPreference = 'Stop'
$session = (Get-Process -Id $PID).SessionId
$file = Join-Path $env:MEETILY_FOCUS_DIR 'meeting-focus-apps.json'
$apps = @(
    @{ name='ChatGPT'; family='OpenAI.Codex_2p2nqsd0c76g0'; id='OpenAI.Codex_2p2nqsd0c76g0!App'; exe='ChatGPT.exe' },
    @{ name='Claude'; family='Claude_pzs8sxrjxfjjc'; id='Claude_pzs8sxrjxfjjc!Claude'; exe='Claude.exe' },
    @{ name='Gemini'; root=(Join-Path $env:LOCALAPPDATA 'Google\Gemini'); id='com.google.gemini.windows'; exe='Gemini.exe' }
)
$receipt = @()
if (Test-Path -LiteralPath $file) {
    $receipt = @(ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($file)) | ForEach-Object { $_ })
}
function Save-Receipt($ids) {
    if ($ids.Count) {
        [IO.File]::WriteAllText("$file.tmp", (ConvertTo-Json -InputObject @($ids) -Compress))
    }
    # A concurrent reader or scanner can briefly deny delete-sharing on Windows.
    for ($attempt = 0; ; $attempt++) {
        try {
            if (!$ids.Count) {
                [IO.File]::Delete($file)
                [IO.File]::Delete("$file.tmp")
            }
            elseif ([IO.File]::Exists($file)) { [IO.File]::Replace("$file.tmp", $file, [System.Management.Automation.Language.NullString]::Value) }
            else { [IO.File]::Move("$file.tmp", $file) }
            return
        } catch [IO.IOException] {
            if ($attempt -ge 10) { throw }
            Start-Sleep -Milliseconds 100
        }
    }
}
foreach ($app in $apps) {
    if ($app.family) {
        $package = Get-AppxPackage -Name ($app.family.Split('_')[0]) | Where-Object PackageFamilyName -eq $app.family | Select-Object -First 1
        $app.root = $package.InstallLocation
    }
}
function Snapshot {
    @(Get-CimInstance Win32_Process -Filter "SessionId=$session" -Property Name,ProcessId,ParentProcessId,ExecutablePath,CreationDate,PrivatePageCount,WorkingSetSize)
}
function Roots($app, $rows) {
    if (!$app.root) { return }
    $prefix = $app.root.TrimEnd('\') + '\'
    $rows | Where-Object {
        $_.Name -eq $app.exe -and $_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)
    }
}
function Process-Tree($app, $rows) {
    $tree = @{}
    foreach ($row in (Roots $app $rows)) { $tree[$row.ProcessId] = $row }
    do {
        $prior = $tree.Count
        foreach ($row in $rows) {
            $parent = $tree[$row.ParentProcessId]
            if ($parent -and $parent.CreationDate -le $row.CreationDate -and
                $row.Name -notin 'chrome.exe','ms-teams.exe','Teams.exe','Raycast.exe','explorer.exe','peacockery-meetily.exe','WindowsTerminal.exe') {
                $tree[$row.ProcessId] = $row
            }
        }
    } while ($tree.Count -gt $prior)
    $tree.Values
}
function Same-Process($row) {
    $p = Get-Process -Id $row.ProcessId -ErrorAction SilentlyContinue
    if ($p -and $p.SessionId -eq $session) {
        # Pin a handle before checking creation time to guard against PID recycling.
        try {
            $null = $p.Handle
            if ([math]::Abs(($p.StartTime.ToUniversalTime() - $row.CreationDate.ToUniversalTime()).TotalMilliseconds) -lt 1) { return $p }
        } catch { if (!$p.HasExited) { throw } }
    }
}
$failures = @()
if ($env:MEETILY_FOCUS_ACTION -eq 'restore') {
    foreach ($id in @($receipt)) {
        $app = $apps | Where-Object { $_.id -eq $id }
        if (!$app) {
            $next = @($receipt | Where-Object { $_ -ne $id })
            Save-Receipt $next
            $receipt = $next
            Write-Output 'Removed retired meeting focus app from restore list'
            continue
        }
        try {
            if ($app.root -and (Test-Path -LiteralPath $app.root) -and !(Roots $app (Snapshot))) {
                Start-Process explorer.exe -ArgumentList ("shell:AppsFolder\" + $app.id)
                $timer = [Diagnostics.Stopwatch]::StartNew()
                do {
                    Start-Sleep -Milliseconds 400
                    $running = @(Roots $app (Snapshot))
                } while (!$running.Count -and $timer.Elapsed.TotalSeconds -lt 15)
                if (!$running.Count) { throw "Launch unconfirmed for $($app.name)" }
            }
            $next = @($receipt | Where-Object { $_ -ne $id })
            Save-Receipt $next
            $receipt = $next
            Write-Output "Cleared restore entry for $($app.name)"
        } catch { $failures += "$($app.name): $_" }
    }
    if ($failures.Count) { throw ($failures -join '; ') }
    return
}
if ($env:MEETILY_FOCUS_ACTION -ne 'close') { throw 'Unknown focus action' }
$rows = Snapshot
$targets = @{}
foreach ($app in $apps) {
    $tree = @(Process-Tree $app $rows)
    if (!$tree.Count) { continue }
    $targets[$app.id] = $tree
    if ($receipt -notcontains $app.id) { $receipt += $app.id }
    $privateMiB = [math]::Round(($tree | Measure-Object PrivatePageCount -Sum).Sum / 1MB)
    Write-Output "$($app.name): $($tree.Count) session processes, $privateMiB MiB private before shutdown"
}
if (!$targets.Count) { return }
Save-Receipt $receipt
foreach ($app in $apps) {
    foreach ($row in (Roots $app $rows)) {
        $p = Same-Process $row
        if ($p -and $p.MainWindowHandle -ne 0) { $null = $p.CloseMainWindow() }
    }
}
Start-Sleep -Milliseconds 1200
foreach ($app in $apps) {
    if (!$targets.ContainsKey($app.id)) { continue }
    try {
        $known = @($targets[$app.id])
        $clear = 0
        for ($attempt = 0; $attempt -lt 6; $attempt++) {
            $known += @(Process-Tree $app (Snapshot))
            foreach ($row in $known) {
                $p = Same-Process $row
                if ($p) { try { $p.Kill() } catch { if (!$p.HasExited) { throw } } }
            }
            Start-Sleep -Milliseconds 300
            $remaining = @($known | Where-Object { Same-Process $_ })
            if (!$remaining.Count -and !(Roots $app (Snapshot))) { $clear++ } else { $clear = 0 }
            if ($clear -ge 2) { break }
            if ($attempt -eq 5) { throw "Processes remain for $($app.name)" }
        }
        Write-Output "Verified $($app.name) exited"
    } catch { $failures += "$($app.name): $_" }
}
if ($failures.Count) { throw ($failures -join '; ') }
