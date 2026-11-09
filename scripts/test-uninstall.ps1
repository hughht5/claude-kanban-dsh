# Tests that uninstalling a test app removes kb.exe, its kb.old-N.exe copies and the install
# folder, and keeps the board's data (card #137).
#
#   powershell -ExecutionPolicy Bypass -File scripts\test-uninstall.ps1 -Channel test2 [-LeaveUninstalled]
#
# Test channels only (test, test2 ... test10); it refuses live. Run it during your turn on that
# test app: it installs and uninstalls it twice, then installs it again (unless -LeaveUninstalled).
# It takes no turn itself, and its uninstalls (unlike install.ps1) don't wait for Hugh or another
# card: take the turn first (a card session: kanban_test_app take; from outside: kb testapp take
# --install --slot <N>, and run this with KANBAN_LOCK_OWNER set to that owner).
#
#   1. installs this checkout into the channel, so its uninstaller has the hook;
#   2. plants a kb.old-N.exe beside kb.exe, quits the test app, runs uninstall.exe /S as Windows
#      does, and checks: the install folder is gone (kb.exe and the copy with it), its uninstall
#      entry is gone, the channel's data folder is still there, and every other Claude Kanban DSH
#      install folder (live and the other slots) is still there;
#   3. installs again, holds kb.exe with a `kb mcp` (as a session's .mcp.json does), uninstalls,
#      and checks the uninstall still finishes within 60 s, with only kb.exe left in the folder;
#      then ends that `kb mcp`, deletes what is left by hand, and checks the folder can go;
#   4. installs it again, so the test app works afterwards.
#
# It prints PASS or FAIL per check and exits 1 if any fails. Written for Windows PowerShell 5.1.

param(
    [Parameter(Mandatory = $true)][string]$Channel,
    [switch]$LeaveUninstalled
)

$ErrorActionPreference = 'Stop'

if ($Channel -notmatch '^test([2-9]|10)?$') {
    Write-Host "Refused: -Channel must be a test channel (test, test2 ... test10), not '$Channel'."
    exit 2
}

$Install = Join-Path $PSScriptRoot 'install.ps1'
$Names   = & powershell -NoProfile -ExecutionPolicy Bypass -File $Install -Channel $Channel -NamesOnly | ConvertFrom-Json
$Dir     = $Names.install_dir
$KbPath  = Join-Path $Dir 'kb.exe'
$Uninst  = Join-Path $Dir 'uninstall.exe'
$Timeout = 60

$script:Failures = 0
function Check([string]$What, [bool]$Ok, [string]$Detail = '') {
    if ($Ok) { Write-Host "PASS $What" } else { $script:Failures++; Write-Host "FAIL $What $Detail" }
}
function Run-Install([string]$Why) {
    Write-Host "--- install.ps1 -Channel $Channel ($Why)"
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install -Channel $Channel
    if ($LASTEXITCODE -ne 0) { throw "install.ps1 failed (exit $LASTEXITCODE) during: $Why" }
    if (-not (Test-Path -LiteralPath $Uninst)) { throw "No uninstall.exe in $Dir after the install" }
}
# Every other install folder of the app: live and the other slots. Only their names are kept.
function Other-Installs {
    @(Get-ChildItem -LiteralPath (Split-Path $Dir -Parent) -Directory -Filter 'Claude Kanban DSH*' -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -ine $Dir } | ForEach-Object { $_.FullName })
}
# The uninstall entries (HKCU, a per-user install) whose install folder is this channel's.
function Uninstall-Entries {
    @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue |
        Where-Object { $u = $_.GetValue('UninstallString'); $u -and $u.Trim('"') -ieq $Uninst })
}
# Asks the test app to quit through its own quit event (as install.ps1 does), so the uninstaller
# doesn't have to force it; the uninstaller stops it anyway if it is still running.
function Quit-App {
    try {
        $ev = [Threading.EventWaitHandle]::OpenExisting($Names.quit_event)
        [void]$ev.Set(); $ev.Dispose()
    } catch { return }
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        if (-not @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -ieq $Names.app_exe }).Count) { return }
        Start-Sleep -Milliseconds 500
    }
    Write-Host 'The test app did not quit within 30 s; the uninstaller will stop it.'
}
# Runs the uninstaller as Windows' Settings does: uninstall.exe copies itself to %TEMP% and runs
# that copy with _?=<the folder>, then exits. Returns whether that copy ended within $Timeout s.
function Run-Uninstall {
    Quit-App
    $p = Start-Process -FilePath $Uninst -ArgumentList '/S' -PassThru
    $p.WaitForExit()
    $deadline = (Get-Date).AddSeconds($Timeout)
    while ((Get-Date) -lt $deadline) {
        # The temp copy's name differs between NSIS versions; its command line always has _?=<dir>.
        $running = @(Get-CimInstance Win32_Process |
            Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf("_?=$Dir", [StringComparison]::OrdinalIgnoreCase) -ge 0 })
        # Done once that copy has ended, and has deleted the app, itself and its uninstall entry
        # (so a poll before the copy has started doesn't count as done).
        if (-not $running.Count -and -not (Test-Path -LiteralPath $Names.app_exe) -and -not (Test-Path -LiteralPath $Uninst) -and
            (Uninstall-Entries).Count -eq 0) { return $true }
        Start-Sleep -Milliseconds 500
    }
    $false
}
function Left-In-Dir {
    @(Get-ChildItem -LiteralPath $Dir -Force -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($Dir.Length + 1) })
}

$Holder = $null
try {
    # 1. This checkout's build, with the hook, in the test app.
    Run-Install 'put this build in place'
    $Others = Other-Installs

    # 2. A plain uninstall, with an old copy beside kb.exe.
    $free = 3..99 | Where-Object { -not (Test-Path -LiteralPath (Join-Path $Dir "kb.old-$_.exe")) } | Select-Object -First 1
    $Planted = Join-Path $Dir "kb.old-$free.exe"
    Copy-Item -LiteralPath $KbPath -Destination $Planted
    Check "planted $(Split-Path $Planted -Leaf) beside kb.exe" (Test-Path -LiteralPath $Planted)
    Check 'the install has an uninstall entry' ((Uninstall-Entries).Count -ge 1)
    $ended = Run-Uninstall
    Check "the uninstall finished within $Timeout s" $ended
    Check 'the install folder is gone (kb.exe and the kb.old-N.exe with it)' (-not (Test-Path -LiteralPath $Dir)) "left: $((Left-In-Dir) -join ', ')"
    Check 'its uninstall entry is gone' ((Uninstall-Entries).Count -eq 0)
    Check "the data folder $($Names.data_dir) is still there" (Test-Path -LiteralPath (Join-Path $Names.data_dir 'board.db'))
    $Lost = @($Others | Where-Object { -not (Test-Path -LiteralPath $_) })
    Check 'live and the other test apps are still installed' ($Lost.Count -eq 0) "gone: $($Lost -join ', ')"

    # 3. An uninstall while a session's `kb mcp` holds kb.exe.
    Run-Install 'install again for the held run'
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $KbPath
    $psi.Arguments = 'mcp'
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    # Only stdin is a pipe (as test-kb-locked.ps1): kb mcp writes nothing until it is sent a message.
    $psi.RedirectStandardInput = $true
    # Not the install folder: a process's working folder can't be removed either, and a real
    # session's isn't there.
    $psi.WorkingDirectory = [IO.Path]::GetTempPath()
    $psi.EnvironmentVariables['KANBAN_DATA_DIR'] = $Names.data_dir
    foreach ($v in 'KANBAN_SESSION', 'KANBAN_CARD') { if ($psi.EnvironmentVariables.ContainsKey($v)) { $psi.EnvironmentVariables.Remove($v) } }
    $Holder = [Diagnostics.Process]::Start($psi)
    Start-Sleep -Milliseconds 800
    Check 'a kb mcp holds the installed kb.exe' (-not $Holder.HasExited)
    $ended = Run-Uninstall
    Check "with kb.exe held, the uninstall still finished within $Timeout s" $ended
    $Left = Left-In-Dir
    # kb.exe (held here) stays; so may a kb.old-N.exe some other session still runs. Nothing else.
    $Other = @($Left | Where-Object { $_ -ine 'kb.exe' -and $_ -notmatch '^kb\.old-\d+\.exe$' })
    Check 'only the held kb.exe (and kb copies still in use) is left in the folder' (($Left -contains 'kb.exe') -and $Other.Count -eq 0) "left: $($Left -join ', ')"
    Check 'the held kb mcp is still running' (-not $Holder.HasExited)
    $Holder.StandardInput.Close()
    if (-not $Holder.WaitForExit(10000)) { $Holder.Kill(); [void]$Holder.WaitForExit(5000) }
    $Holder = $null
    Remove-Item -LiteralPath $KbPath -Force
    Get-ChildItem -LiteralPath $Dir -Filter 'kb.old-*.exe' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $Dir -Force -ErrorAction SilentlyContinue
    Check 'once that kb mcp ended, kb.exe and the folder could be deleted' (-not (Test-Path -LiteralPath $Dir))
} catch {
    $script:Failures++
    Write-Host "FAIL $($_.Exception.Message)"
} finally {
    if ($Holder -and -not $Holder.HasExited) { try { $Holder.Kill(); [void]$Holder.WaitForExit(5000) } catch {} }
    if (-not $LeaveUninstalled) {
        try { Run-Install 'put the test app back' } catch { Write-Host "The test app is not installed again: $($_.Exception.Message)" }
    }
}

if ($script:Failures -gt 0) { Write-Host "$($script:Failures) check(s) failed"; exit 1 }
Write-Host 'All checks passed'
