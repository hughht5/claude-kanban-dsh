# Tests that an install replaces kb.exe while a `kb mcp` holds it open (card #126).
#
#   powershell -ExecutionPolicy Bypass -File scripts\test-kb-locked.ps1 [-Channel test]
#
# Test channels only (test, test2 ... test10); it refuses live. Run it during your test-app turn:
# it installs three times with install.ps1 -Channel <c>, which quits and restarts that test app.
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
# It prints PASS or FAIL per check and exits 1 if any fails. Written for Windows PowerShell 5.1.

param([string]$Channel = 'test')

$ErrorActionPreference = 'Stop'

if ($Channel -notmatch '^test([2-9]|10)?$') {
    Write-Host "Refused: -Channel must be a test channel (test, test2 ... test10), not '$Channel'."
    exit 2
}

$Root    = Split-Path $PSScriptRoot -Parent
$Install = Join-Path $PSScriptRoot 'install.ps1'
$Names   = & powershell -NoProfile -ExecutionPolicy Bypass -File $Install -Channel $Channel -NamesOnly | ConvertFrom-Json
$Dir     = $Names.install_dir
$KbPath  = Join-Path $Dir 'kb.exe'
$Target  = if ($Names.slot -eq 1) { 'target\test' } else { "target\test$($Names.slot)" }
$KbBuilt = Join-Path $Root "$Target\release\kb.exe"

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
$Holder = $null
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

    # 4. Install over it.
    Run-Install 'replace the held kb.exe'
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
} catch {
    $script:Failures++
    Write-Host "FAIL $($_.Exception.Message)"
} finally {
    if ($Holder -and -not $Holder.HasExited) { try { $Holder.Kill(); [void]$Holder.WaitForExit(5000) } catch {} }
    # Never leave the planted kb.exe (the build plus a byte) in the test app.
    if ($BuiltHash -and (Test-Path -LiteralPath $KbPath) -and (Hash $KbPath) -ne $BuiltHash) {
        try { Put-Kb $KbBuilt; Write-Host 'Put the built kb.exe back in place of the planted one.' }
        catch { Write-Host "The test app's kb.exe is not the build; run install.ps1 -Channel $Channel to fix it. ($($_.Exception.Message))" }
    }
}

if ($script:Failures -gt 0) { Write-Host "$($script:Failures) check(s) failed"; exit 1 }
Write-Host 'All checks passed'
