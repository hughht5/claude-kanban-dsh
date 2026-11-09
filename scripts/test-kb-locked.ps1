# Tests that an install replaces kb.exe while a `kb mcp` holds it open (card #126), and that
# Restart now does too, or says so when it can't (card #136).
#
#   powershell -ExecutionPolicy Bypass -File scripts\test-kb-locked.ps1 [-Channel test] [-Restart | -Fail]
#
# Test channels only (test, test2 ... test10); it refuses live. Run it during your test-app turn:
# it installs with install.ps1 -Channel <c>, which quits and restarts that test app.
#
# With no switch it tests install.ps1:
#
#   1. installs this checkout into the channel, so its installer has the fix;
#   2. swaps the installed kb.exe for one that differs from the build (the built one with a byte
#      added: a rebuild of one commit is byte-identical, so without this a broken hook would still
#      leave a matching kb.exe). If a session holds the installed one, it is renamed aside first;
#   3. starts a `kb mcp` from it, windowless, with stdin kept open, as a session's .mcp.json does,
#      and checks kb.exe really can't be deleted;
#   4. installs again, and checks kb.exe now matches the build, the held copy is a new
#      kb.old-N.exe, and the held `kb mcp` is still running;
#   5. ends that `kb mcp`, installs a third time, and checks that kb.old-N.exe is gone.
#
# install.ps1 copies kb.exe again itself when the installer left none; Restart now's helper
# doesn't, so only a Restart now shows what the installer's hook did alone. The two switches
# test it (card #136). Each stages this build (install.ps1 -Stage), clicks Restart now over the
# test app's DevTools port (tools\cdp.mjs: no window comes to the front), waits for the app to
# come back, and checks it was started with --background:
#
#   -Restart  steps 1 to 3, then Restart now in place of step 4: kb.exe matches the build, the
#             held one is a new kb.old-N.exe, the `kb mcp` still runs, the result file says ok
#             and the board shows no notice. Then step 5.
#   -Fail     steps 1 and 2; then the script itself opens the planted kb.exe with no write or
#             delete sharing, so the installer can neither rename nor overwrite it and exits 3
#             (a running `kb mcp` can't cause that: Windows lets a running exe be renamed). After
#             Restart now, the planted kb.exe is still there, no kb.old-N.exe was made, the
#             result file has kind "kb" and names this script's process, and the board shows
#             "Updated, but kb.exe is still the old version" with that holder until its OK.
#
# It prints PASS or FAIL per check and exits 1 if any fails. Written for Windows PowerShell 5.1.

param([string]$Channel = 'test', [switch]$Restart, [switch]$Fail)

$ErrorActionPreference = 'Stop'

if ($Channel -notmatch '^test([2-9]|10)?$') {
    Write-Host "Refused: -Channel must be a test channel (test, test2 ... test10), not '$Channel'."
    exit 2
}
if ($Restart -and $Fail) {
    Write-Host 'Refused: pass -Restart or -Fail, not both.'
    exit 2
}

$Root    = Split-Path $PSScriptRoot -Parent
$Install = Join-Path $PSScriptRoot 'install.ps1'
$Names   = & powershell -NoProfile -ExecutionPolicy Bypass -File $Install -Channel $Channel -NamesOnly | ConvertFrom-Json
$Dir     = $Names.install_dir
$KbPath  = Join-Path $Dir 'kb.exe'
$Target  = if ($Names.slot -eq 1) { 'target\test' } else { "target\test$($Names.slot)" }
$KbBuilt = Join-Path $Root "$Target\release\kb.exe"
$UpdateDir = Join-Path $Names.data_dir 'update'
$Cdp     = Join-Path $Root 'tools\cdp.mjs'

$script:Failures = 0
function Check([string]$What, [bool]$Ok, [string]$Detail = '') {
    if ($Ok) { Write-Host "PASS $What" } else { $script:Failures++; Write-Host "FAIL $What $Detail" }
}
function Hash([string]$Path) { (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash }
function Old-Copies {
    $h = @{}
    Get-ChildItem -LiteralPath $Dir -Filter 'kb.old-*.exe' -ErrorAction SilentlyContinue |
        ForEach-Object { $h[$_.FullName] = Hash $_.FullName }
    $h
}
# Puts $Source in as kb.exe even if a session runs the installed one: the same rename-aside the
# installer hook does.
function Put-Kb([string]$Source) {
    try { Remove-Item -LiteralPath $KbPath -Force -ErrorAction Stop } catch {
        $free = 1..99 | Where-Object { -not (Test-Path -LiteralPath (Join-Path $Dir "kb.old-$_.exe")) } | Select-Object -First 1
        if (-not $free) { throw 'kb.exe is in use and kb.old-1..99.exe are all taken; end some sessions and run again' }
        Rename-Item -LiteralPath $KbPath -NewName "kb.old-$free.exe"
    }
    Copy-Item -LiteralPath $Source -Destination $KbPath
}
# A process runs from this exact file: what makes Windows refuse to delete or overwrite it.
function Runs-From([string]$Path) {
    @(Get-CimInstance Win32_Process -Filter "Name='kb.exe'" | Where-Object { $_.ExecutablePath -ieq $Path }).Count -gt 0
}
function Run-Install([string]$Why) {
    Write-Host "--- install.ps1 -Channel $Channel ($Why)"
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install -Channel $Channel
    if ($LASTEXITCODE -ne 0) { throw "install.ps1 failed (exit $LASTEXITCODE) during: $Why" }
}

# --- Restart now over the test app's DevTools port (card #136) ------------------------------

# Runs JavaScript in the test app's page and returns its value, or $null when the page doesn't
# answer (the app is down or still starting). Single quotes only in $Js: Windows PowerShell 5.1
# mangles double quotes in a native command's arguments.
function Cdp([string]$Js) {
    $env:CDP_PORT = [string]$Names.devtools_port
    $out = $null
    try { $out = & node $Cdp eval $Js 2>$null } catch { return $null }
    if ($LASTEXITCODE -ne 0 -or -not $out) { return $null }
    try { ($out -join "`n") | ConvertFrom-Json } catch { $null }
}
# The update bar as the page shows it: hidden or not, its text, and which button it has.
function Bar {
    $b = Cdp "(function(){var b=document.getElementById('updateBar');if(!b)return {missing:true};return {hidden:b.hidden,text:b.innerText,now:!!document.getElementById('updNow'),ok:!!document.getElementById('updOk')}})()"
    if ($b -and -not $b.missing) { $b } else { $null }
}
function App-Procs {
    @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -ieq $Names.app_exe })
}
# The newest of restart-result.json and restart-result.seen.json (the app renames the first to the
# second as it reads it; the time stays), if it was written after $Since (UTC).
function Restart-Result([datetime]$Since) {
    $f = Get-ChildItem -LiteralPath $UpdateDir -Filter 'restart-result*.json' -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTimeUtc -gt $Since } | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    if (-not $f) { return $null }
    $bytes = [IO.File]::ReadAllBytes($f.FullName)
    $bom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
    Check 'the result file has no byte-order mark' (-not $bom)
    [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json
}
# Waits up to $Seconds for the bar to have (or not have) its OK button; returns the bar.
function Wait-Notice([bool]$Want, [int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $bar = Bar
        if ($bar -and [bool]$bar.ok -eq $Want) { return $bar }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    $bar
}
# Stages this build, clicks Restart now in the test app, and waits until it is back. Returns the
# helper's result (ok, kind, error, at).
function Restart-Now {
    Write-Host "--- install.ps1 -Channel $Channel -Stage (this build, for Restart now)"
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install -Channel $Channel -Stage
    if ($LASTEXITCODE -ne 0) { throw "install.ps1 -Stage failed (exit $LASTEXITCODE)" }
    $staged = Get-Content -Raw -LiteralPath (Join-Path $UpdateDir 'update.json') | ConvertFrom-Json
    Check 'update.json asks for a background restart' ($staged.background -eq $true)

    Write-Host '--- waiting for Update ready in the test app (it looks every 30 s)'
    $deadline = (Get-Date).AddSeconds(90)
    do {
        $bar = Bar
        if ($bar -and $bar.now) { break }
        Start-Sleep -Seconds 3
    } while ((Get-Date) -lt $deadline)
    if (-not ($bar -and $bar.now)) { throw "the test app showed no Restart now within 90 s (is it running, with port $($Names.devtools_port)?)" }
    $old = @(App-Procs | ForEach-Object { [int]$_.ProcessId })
    $since = (Get-Date).ToUniversalTime().AddSeconds(-2)
    $clicked = Cdp "(function(){document.getElementById('updNow').click();return 'clicked'})()"
    if ($clicked -ne 'clicked') { throw 'clicking Restart now over CDP failed' }
    Write-Host '--- clicked Restart now; waiting for the app to quit, install and come back'

    foreach ($id in $old) { Wait-Process -Id $id -Timeout 120 -ErrorAction SilentlyContinue }
    Check 'the app quit for Restart now' (@(App-Procs | Where-Object { $old -contains [int]$_.ProcessId }).Count -eq 0)
    $deadline = (Get-Date).AddMinutes(5)
    $result = $null
    $new = @()
    do {
        Start-Sleep -Seconds 3
        if (-not $result) { $result = Restart-Result $since }
        $new = @(App-Procs | Where-Object { $old -notcontains [int]$_.ProcessId })
    } while ((-not $result -or $new.Count -eq 0) -and (Get-Date) -lt $deadline)
    if (-not $result) { throw 'the helper wrote no restart-result.json within 5 minutes (see update\restart.log)' }
    Check 'the app came back after Restart now' ($new.Count -gt 0)
    $bg = @($new | Where-Object { $_.CommandLine -match '--background' }).Count -gt 0
    Check 'it came back with --background (never takes focus)' $bg "command line: $(@($new | ForEach-Object { $_.CommandLine }) -join ' | ')"
    $start = Get-Content -LiteralPath (Join-Path $UpdateDir 'restart.log') -ErrorAction SilentlyContinue |
        Where-Object { $_ -match ' Starting ' } | Select-Object -Last 1
    Check 'restart.log says it started it with --background' ([string]$start -match '--background') "line: $start"

    # The page is up once it answers; its update bar reads the result as it loads.
    $deadline = (Get-Date).AddSeconds(60)
    do {
        Start-Sleep -Seconds 2
        $bar = Bar
    } while (-not $bar -and (Get-Date) -lt $deadline)
    if (-not $bar) { throw "the restarted app's page didn't answer on port $($Names.devtools_port) within 60 s" }
    $result
}

$Holder = $null
$Lock = $null
$BuiltHash = $null
try {
    # 1. This checkout's build, with the fixed hook, in the test app.
    Run-Install 'put the fixed installer in place'
    if (-not (Test-Path -LiteralPath $KbBuilt)) { throw "No built kb.exe at $KbBuilt" }
    $BuiltHash = Hash $KbBuilt

    # 2. A kb.exe that differs from the build.
    $Planted = Join-Path ([IO.Path]::GetTempPath()) ("kb-planted-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.exe')
    Copy-Item -LiteralPath $KbBuilt -Destination $Planted
    $fs = [IO.File]::Open($Planted, 'Append', 'Write'); $fs.WriteByte(0); $fs.Dispose()
    $PlantedHash = Hash $Planted
    Put-Kb $Planted
    Remove-Item -LiteralPath $Planted -Force
    Check 'the planted kb.exe differs from the build' ((Hash $KbPath) -ne $BuiltHash)
    $Before = Old-Copies

    if ($Fail) {
        # 3. Hold it so it can be neither renamed nor overwritten: open for reading, sharing only
        # reads (no write or delete sharing). The installer then exits 3.
        $Lock = [IO.File]::Open($KbPath, 'Open', 'Read', 'Read')
        $renamed = $false
        try { Rename-Item -LiteralPath $KbPath -NewName 'kb.renametest.exe' -ErrorAction Stop; $renamed = $true } catch {}
        if ($renamed) { Rename-Item -LiteralPath (Join-Path $Dir 'kb.renametest.exe') -NewName 'kb.exe' }
        Check 'kb.exe, open with no delete sharing, cannot be renamed' (-not $renamed)

        # 4. Restart now over it.
        $r = Restart-Now
        $After = Old-Copies
        $New = @($After.Keys | Where-Object { -not $Before.ContainsKey($_) })
        Check 'the result says kb.exe was not replaced (ok false, kind "kb")' ($r.ok -eq $false -and $r.kind -eq 'kb') "result: $($r | ConvertTo-Json -Compress)"
        Check 'the result names this script as the holder' ([string]$r.error -match "\(pid $PID\)") "error: $($r.error)"
        Check 'kb.exe is still the planted one (left as it was, not missing)' ((Test-Path -LiteralPath $KbPath) -and (Hash $KbPath) -eq $PlantedHash)
        Check 'no kb.old-N.exe was made' ($New.Count -eq 0) "new: $($New -join ', ')"
        $bar = Wait-Notice $true 40
        Check 'the board shows the kb.exe notice' ($bar -and $bar.ok -and $bar.text -match 'kb\.exe is still the old version') "bar: $($bar | ConvertTo-Json -Compress)"
        Check 'the notice names the holder' ($bar -and $bar.text -match "pid $PID") "bar text: $($bar.text)"
        [void](Cdp "(function(){var b=document.getElementById('updOk');if(b)b.click();return 'ok'})()")
        $bar = Wait-Notice $false 15
        Check 'OK clears the notice' ($bar -and -not $bar.ok -and $bar.hidden) "bar: $($bar | ConvertTo-Json -Compress)"
        $Lock.Dispose(); $Lock = $null
    } else {
        # 3. Hold it, as a session's `kb mcp` does: no window, stdin open, this channel's board.
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $KbPath
        $psi.Arguments = 'mcp'
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $true
        # Only stdin is a pipe: kb mcp writes nothing until it is sent a message, and an unread
        # stdout pipe could fill and block it.
        $psi.RedirectStandardInput = $true
        $psi.WorkingDirectory = $Dir
        $psi.EnvironmentVariables['KANBAN_DATA_DIR'] = $Names.data_dir
        foreach ($v in 'KANBAN_SESSION', 'KANBAN_CARD') { if ($psi.EnvironmentVariables.ContainsKey($v)) { $psi.EnvironmentVariables.Remove($v) } }
        $Holder = [Diagnostics.Process]::Start($psi)
        Start-Sleep -Milliseconds 800
        Check 'the holding kb mcp is running' (-not $Holder.HasExited)
        Check 'kb.exe is held (a kb mcp runs from it, so it cannot be overwritten)' (Runs-From $KbPath)
        $locked = $false
        try { [IO.File]::Open($KbPath, 'Open', 'Write', 'ReadWrite').Dispose() } catch { $locked = $true }
        Check 'kb.exe cannot be opened for writing' $locked

        # 4. Install over it, or with -Restart, Restart now over it.
        if ($Restart) {
            $r = Restart-Now
            Check 'the result says the update installed (ok true)' ($r.ok -eq $true) "result: $($r | ConvertTo-Json -Compress)"
            $bar = Wait-Notice $false 10
            Check 'the board shows no notice' ($bar -and -not $bar.ok) "bar: $($bar | ConvertTo-Json -Compress)"
        } else {
            Run-Install 'replace the held kb.exe'
        }
        $After = Old-Copies
        $New = @($After.Keys | Where-Object { -not $Before.ContainsKey($_) -and $After[$_] -eq $PlantedHash })
        Check 'kb.exe now matches the build (SHA-256)' ((Hash $KbPath) -eq $BuiltHash) "installed $(Hash $KbPath), built $BuiltHash"
        Check 'the held kb.exe is now a new kb.old-N.exe' ($New.Count -eq 1) "new copies with the planted hash: $($New.Count)"
        Check 'the holding kb mcp is still running' (-not $Holder.HasExited)

        # 5. End the holder; the next install clears its copy.
        $Holder.StandardInput.Close()
        if (-not $Holder.WaitForExit(10000)) { $Holder.Kill() }
        $Holder = $null
        Run-Install 'clear the old copy'
        $Gone = @($New | Where-Object { Test-Path -LiteralPath $_ })
        Check 'the next install removed that kb.old-N.exe' ($New.Count -eq 1 -and $Gone.Count -eq 0) "still there: $($Gone -join ', ')"
    }
} catch {
    $script:Failures++
    Write-Host "FAIL $($_.Exception.Message)"
} finally {
    if ($Lock) { $Lock.Dispose() }
    if ($Holder -and -not $Holder.HasExited) { try { $Holder.Kill(); [void]$Holder.WaitForExit(5000) } catch {} }
    # Never leave the planted kb.exe (the build plus a byte) in the test app.
    if ($BuiltHash -and (Test-Path -LiteralPath $KbPath) -and (Hash $KbPath) -ne $BuiltHash) {
        try { Put-Kb $KbBuilt; Write-Host 'Put the built kb.exe back in place of the planted one.' }
        catch { Write-Host "The test app's kb.exe is not the build; run install.ps1 -Channel $Channel to fix it. ($($_.Exception.Message))" }
    }
}

if ($script:Failures -gt 0) { Write-Host "$($script:Failures) check(s) failed"; exit 1 }
Write-Host 'All checks passed'
