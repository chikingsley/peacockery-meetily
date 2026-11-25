# Runs in the recorder's interactive session. Only these Start menu applications participate.
$ErrorActionPreference = 'Stop'
$apps = @(
    @{ process = 'ChatGPT'; id = 'OpenAI.Codex_2p2nqsd0c76g0!App' },
    @{ process = 'Claude'; id = 'Claude_pzs8sxrjxfjjc!Claude' },
    @{ process = 'Gemini'; id = 'com.google.gemini.windows' }
)
$file = Join-Path $env:MEETILY_FOCUS_DIR 'meeting-focus-apps.json'
$session = (Get-Process -Id $PID).SessionId
if ($env:MEETILY_FOCUS_ACTION -eq 'restore') {
    if (!(Test-Path -LiteralPath $file)) { return }
    $targets = @(Get-Content -Raw -LiteralPath $file | ConvertFrom-Json)
    foreach ($app in $apps) {
        if ($targets -contains $app.id) {
            Start-Process explorer.exe -ArgumentList ("shell:AppsFolder\" + $app.id)
        }
    }
    Remove-Item -LiteralPath $file
    return
}
$installed = @(Get-StartApps | Select-Object -ExpandProperty AppID)
$windows = @()
$targets = @()
foreach ($app in $apps) {
    if ($installed -notcontains $app.id) { continue }
    $visible = @(Get-Process -Name $app.process -ErrorAction SilentlyContinue | Where-Object {
        $_.SessionId -eq $session -and $_.MainWindowHandle -ne 0
    })
    if ($visible.Count) {
        $targets += $app.id
        $windows += $visible
    }
}
if (!$targets.Count) { return }
ConvertTo-Json -InputObject $targets -Compress | Set-Content -Encoding UTF8 -LiteralPath ($file + '.tmp')
Move-Item -LiteralPath ($file + '.tmp') -Destination $file -Force
foreach ($window in $windows) {
    if (!$window.HasExited) { $null = $window.CloseMainWindow() }
}
