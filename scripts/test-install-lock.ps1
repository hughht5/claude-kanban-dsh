# Tests the turn-taking for test installs in scripts\install.ps1, with -LockOnly.
#
#   powershell -ExecutionPolicy Bypass -File scripts\test-install-lock.ps1
#
# Each case runs `install.ps1 -Channel test -LockOnly` (or -Channel test2, test3 for the slots of
# card #147) as a child powershell, with -LockDir and -DshLockDir in a new temp folder, so it
# never touches the real shared folder (%USERPROFILE%\.claude-kanban-dsh-shared), the DSH lock
# folder or an app: no build, no install, nothing is quit or started. The test writes the apps'
# files (turn-*.json, old and new shape, hold.json, input-<channel>.json) itself. It prints PASS
# or FAIL per check and exits 1 if any fails. Written for Windows PowerShell 5.1.

$ErrorActionPreference = 'Stop'

$Install  = Join-Path $PSScriptRoot 'install.ps1'
$Base     = Join-Path ([IO.Path]::GetTempPath()) ('kanban-install-lock-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $Base | Out-Null
$script:Failures = 0
$script:Passes   = 0
$script:CaseNo   = 0
$script:RunNo    = 0
$Inv = [Globalization.CultureInfo]::InvariantCulture

function Format-Utc([datetime]$Time) { $Time.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", $Inv) }
function Utc([double]$Minutes = 0) { Format-Utc ([datetime]::UtcNow.AddMinutes($Minutes)) }

# This PowerShell stands in for a running app: its pid and start time.
$MyStart = Format-Utc (Get-Process -Id $PID).StartTime

function New-Case {
    $script:CaseNo++
    $dir = Join-Path $Base "case$($script:CaseNo)"
    New-Item -ItemType Directory -Path $dir | Out-Null
    [pscustomobject]@{
        Dir    = $dir
        Shared = Join-Path $dir 'shared'
        Dsh    = Join-Path $dir 'dsh-test-app.lock'
        Lock   = Join-Path $dir 'shared\install.lock'
    }
}

function Write-Fixture($Case, [string]$Name, $Object) {
    New-Item -ItemType Directory -Force -Path $Case.Shared | Out-Null
    [IO.File]::WriteAllText((Join-Path $Case.Shared $Name), ($Object | ConvertTo-Json -Depth 5))
}

function New-Turn([string]$Channel, $AppPid, [string]$AppStart, $Holder) {
    [ordered]@{ channel = $Channel; appPid = $AppPid; appStart = $AppStart; writtenAt = (Utc); holder = $Holder }
}

function New-Holder([string]$Owner, [string]$Label, [string]$Kind = 'tester', $Card = $null) {
    [ordered]@{ kind = $Kind; card = $Card; owner = $Owner; label = $Label; givenAt = (Utc -1); expiresAt = (Utc 10) }
}

# A turn file in card #147's shape: `holder` (slot 1's, for older scripts) and `slots`, an
# ordered map of channel -> New-Slot.
function New-SlotTurn([string]$Channel, $AppPid, [string]$AppStart, $Holder, $Slots) {
    [ordered]@{ channel = $Channel; appPid = $AppPid; appStart = $AppStart; writtenAt = (Utc); holder = $Holder; slots = $Slots }
}

# One slot in `slots`: the card it is given to (or $null) and its turn's holder (or $null).
function New-Slot([int]$Slot, $Card = $null, $Holder = $null) {
    $label = $null
    if ($null -ne $Card) { $label = "#$Card" }
    [ordered]@{ slot = $Slot; card = $Card; label = $label; holder = $Holder }
}

function Read-Shared([string]$Path) {
    try {
        $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
        try { return (New-Object IO.StreamReader($fs)).ReadToEnd() } finally { $fs.Dispose() }
    } catch { return '' }
}

function Read-SharedJson([string]$Path) {
    try { return ((Read-Shared $Path) | ConvertFrom-Json) } catch { return $null }
}

# Holds install.lock the way install.ps1 does, with a JSON body.
function Open-HeldLock([string]$Path, $Object) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    $s = [IO.FileStream]::new($Path, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite,
        [IO.FileShare]'Read, Delete', 4096, [IO.FileOptions]::DeleteOnClose)
    $bytes = (New-Object Text.UTF8Encoding $false).GetBytes(($Object | ConvertTo-Json -Compress))
    $s.Write($bytes, 0, $bytes.Length)
    $s.SetLength($bytes.Length)
    $s.Flush()
    return $s
}

# Starts install.ps1 -Channel <test> -LockOnly as a child powershell (no new window).
function Start-Run($Case, [string[]]$Extra = @(), [string]$Owner = 'tester', [string]$Channel = 'test', [string]$DataDir = '') {
    $script:RunNo++
    $out = Join-Path $Case.Dir "run$($script:RunNo).out.txt"
    $err = Join-Path $Case.Dir "run$($script:RunNo).err.txt"
    $argv = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$Install`"", '-Channel', $Channel, '-LockOnly',
        '-LockDir', "`"$($Case.Shared)`"", '-DshLockDir', "`"$($Case.Dsh)`"", '-PollSeconds', '1') + $Extra
    $savedOwner = $env:KANBAN_LOCK_OWNER
    $savedCard  = $env:KANBAN_CARD
    $savedData  = $env:KANBAN_DATA_DIR
    try {
        $env:KANBAN_LOCK_OWNER = $Owner
        Remove-Item Env:KANBAN_CARD -ErrorAction SilentlyContinue
        # The run's board (card #147): live unless the case names a test app's data folder.
        if ($DataDir) { $env:KANBAN_DATA_DIR = $DataDir } else { Remove-Item Env:KANBAN_DATA_DIR -ErrorAction SilentlyContinue }
        $p = Start-Process -FilePath 'powershell.exe' -ArgumentList ($argv -join ' ') -NoNewWindow -PassThru `
            -RedirectStandardOutput $out -RedirectStandardError $err
    } finally {
        if ($null -eq $savedOwner) { Remove-Item Env:KANBAN_LOCK_OWNER -ErrorAction SilentlyContinue } else { $env:KANBAN_LOCK_OWNER = $savedOwner }
        if ($null -ne $savedCard) { $env:KANBAN_CARD = $savedCard }
        if ($null -eq $savedData) { Remove-Item Env:KANBAN_DATA_DIR -ErrorAction SilentlyContinue } else { $env:KANBAN_DATA_DIR = $savedData }
    }
    $null = $p.Handle   # so ExitCode is kept after it exits (Windows PowerShell 5.1)
    [pscustomobject]@{ Proc = $p; Out = $out; Err = $err; Started = Get-Date }
}

function Wait-Run($Run, [int]$TimeoutSeconds = 40) {
    $exited = $Run.Proc.WaitForExit($TimeoutSeconds * 1000)
    if (-not $exited) {
        try { $Run.Proc.Kill() } catch { }
        [void]$Run.Proc.WaitForExit(5000)
    }
    $Run.Proc.WaitForExit()
    $exit = 'timeout'
    if ($exited) { $exit = $Run.Proc.ExitCode }
    $errText = Read-Shared $Run.Err
    [pscustomobject]@{
        Exit     = $exit
        TimedOut = -not $exited
        Seconds  = [math]::Round(((Get-Date) - $Run.Started).TotalSeconds, 1)
        Text     = (Read-Shared $Run.Out) + $errText
        ErrText  = $errText
    }
}

# A child powershell can take several seconds to start, so wait for its "Waiting:" line
# rather than for a fixed time. $true when it printed $Pattern and still runs $ThenSeconds later.
function Wait-Waiting($Run, [string]$Pattern, [int]$ThenSeconds = 2, [int]$TimeoutSeconds = 40) {
    $until = (Get-Date).AddSeconds($TimeoutSeconds)
    $seen = $false
    while (-not $seen -and (Get-Date) -lt $until -and -not $Run.Proc.HasExited) {
        $seen = (Read-Shared $Run.Out) -match [regex]::Escape($Pattern)
        if (-not $seen) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $seen) { return $false }
    if ($ThenSeconds -gt 0) { Start-Sleep -Seconds $ThenSeconds }
    return (-not $Run.Proc.HasExited)
}

function Get-UtcOrNull($Value) {
    $t = [datetime]::MinValue
    if ([datetime]::TryParse([string]$Value, $Inv, [Globalization.DateTimeStyles]'AdjustToUniversal, AssumeUniversal', [ref]$t)) { return $t }
    return $null
}

function Get-WaitFile($Case, $Run) { Join-Path $Case.Shared "install-wait-$($Run.Proc.Id).json" }

function Check([string]$Name, [bool]$Ok, $Result = $null) {
    if ($Ok) {
        $script:Passes++
        Write-Host "PASS  $Name"
    } else {
        $script:Failures++
        Write-Host "FAIL  $Name" -ForegroundColor Red
        if ($Result) {
            Write-Host "      exit $($Result.Exit), $($Result.Seconds) s, output:"
            ($Result.Text -split "`r?`n") | Where-Object { $_ } | ForEach-Object { Write-Host "      | $_" }
        }
    }
}

function Test-Clear($Result) { $Result.Exit -eq 0 -and $Result.Text -match 'LOCK OK' -and $Result.Text -notmatch 'Waiting:' }

try {
    # --- 1. Clear: no files ---------------------------------------------------------------
    $c = New-Case
    $r = Wait-Run (Start-Run $c)
    Check '1 clear: exits 0 with LOCK OK, no wait' (Test-Clear $r) $r
    Check '1 clear: the shared folder was made, install.lock is gone (delete-on-close)' ((Test-Path $c.Shared) -and -not (Test-Path $c.Lock)) $r
    Check '1 clear: the DSH lock folder is gone afterwards' (-not (Test-Path $c.Dsh)) $r
    Check '1 clear: it took the DSH lock folder' ($r.Text -match 'Took the DSH lock folder .* \(owner tester\)') $r

    # The run printed a Waiting: install.lock line, and LOCK OK only after it.
    function Test-WaitedThenOk($Run) {
        $w = $Run.Text.IndexOf('Waiting: install.lock')
        $ok = $Run.Text.LastIndexOf('LOCK OK')
        return ($w -ge 0 -and $ok -gt $w)
    }

    # --- 2. A held install.lock -----------------------------------------------------------
    $c = New-Case
    $held = Open-HeldLock $c.Lock ([ordered]@{ pid = $PID; owner = 'other'; label = 'held by the test'; phase = 'installing'; waitingFor = $null })
    $run = Start-Run $c
    $still = Wait-Waiting $run 'Waiting: install.lock'
    $held.Dispose()
    $r = Wait-Run $run
    Check '2 held: waits while another process holds install.lock, says who' ($still -and $r.Text -match 'Waiting: install.lock, held by held by the test \(owner other') $r
    # Order, not time: a busy machine makes any duration check flaky. It was still waiting when
    # the lock was let go, and its LOCK OK comes after its Waiting line.
    Check '2 held: LOCK OK once it is let go' ($r.Exit -eq 0 -and $still -and (Test-WaitedThenOk $r)) $r

    $c = New-Case
    $held = Open-HeldLock $c.Lock ([ordered]@{ pid = $PID; owner = 'other'; label = 'never lets go'; phase = 'installing' })
    $r = Wait-Run (Start-Run $c @('-LockWaitMinutes', '0.05'))
    $held.Dispose()
    Check '2 held for ever: fails after -LockWaitMinutes (0.05) with a clear message' (
        $r.Exit -ne 0 -and -not $r.TimedOut -and $r.Text -match 'Gave up after 0.05 min waiting for install.lock, held by never lets go' -and $r.Text -notmatch 'LOCK OK') $r

    # --- 3. Delete-pending: a reader has it open as the holder closes ---------------------
    $c = New-Case
    $held = Open-HeldLock $c.Lock ([ordered]@{ pid = $PID; owner = 'other'; label = 'closing'; phase = 'started' })
    $run = Start-Run $c
    [void](Wait-Waiting $run 'Waiting: install.lock' 0)
    $reader = [IO.File]::Open($c.Lock, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
    $held.Dispose()
    $probe = 'opened'
    try {
        $x = [IO.FileStream]::new($c.Lock, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite,
            [IO.FileShare]'Read, Delete', 4096, [IO.FileOptions]::DeleteOnClose)
        $x.Dispose()
    } catch {
        $e = $_.Exception
        if ($e.InnerException) { $e = $e.InnerException }
        $probe = $e.GetType().Name
    }
    Start-Sleep -Seconds 3
    $reader.Dispose()
    $r = Wait-Run $run
    Check '3 delete-pending: retries instead of throwing, then LOCK OK' (
        $r.Exit -eq 0 -and $r.ErrText -notmatch '\S' -and (Test-WaitedThenOk $r)) $r
    Write-Host "      (a new open while the reader held the closed file gave: $probe)"

    # --- 4. A live turn for another owner -------------------------------------------------
    $c = New-Case
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart (New-Holder '105' '#105' 'tester' 105))
    $run = Start-Run $c
    $still = Wait-Waiting $run "Waiting: the test app's turn"
    $lock = Read-SharedJson (Get-WaitFile $c $run)
    $heldWhileWaiting = Test-Path $c.Lock
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart $null)
    $r = Wait-Run $run
    Check '4 live turn: waits, says whose turn' ($still -and $r.Text -match "Waiting: the test app's turn on Test: #105 \(tester\)") $r
    Check '4 live turn: waits without install.lock' (-not $heldWhileWaiting) $r
    Check '4 live turn: no wait file left afterwards' (-not (Test-Path (Get-WaitFile $c $run))) $r
    Check '4 live turn: install-wait-<pid>.json carries the channel (test)' ($lock -and $lock.channel -eq 'test') $r
    Check '4 live turn: install-wait-<pid>.json says waiting, why, and who' (
        $lock -and $lock.phase -eq 'waiting' -and $lock.waitingFor -eq "the test app's turn on Test: #105 (tester)" -and
        $lock.owner -eq 'tester' -and $lock.label -eq 'tester' -and $lock.pid -gt 0 -and $lock.pidStart -match 'Z$' -and
        $lock.pid -eq $run.Proc.Id -and $null -eq $lock.restartAt -and $lock.startedAt -match 'Z$' -and $null -eq $lock.card -and $lock.worktree) $r
    Check '4 live turn: LOCK OK once the holder is null' ($r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    # A turn file that can't be read or parsed: wait (an install must not go ahead under a tester).
    $c = New-Case
    New-Item -ItemType Directory -Force -Path $c.Shared | Out-Null
    [IO.File]::WriteAllText((Join-Path $c.Shared 'turn-test.json'), '{"channel": "test", "holder": {')
    $run = Start-Run $c
    $still = Wait-Waiting $run "Waiting: the test app's turn file"
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart $null)
    $r = Wait-Run $run
    Check "4 unreadable turn file: waits (the test app's turn file can't be read)" ($still -and $r.Text -match "Waiting: the test app's turn file can't be read") $r
    Check '4 unreadable turn file: LOCK OK once it reads again' ($r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    # Held open without delete sharing (as during a failed rename) also counts as unreadable.
    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-Turn 'live' $PID $MyStart $null)
    $blocker = [IO.File]::Open((Join-Path $c.Shared 'turn-live.json'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $run = Start-Run $c
    $still = Wait-Waiting $run "Waiting: the live app's turn file"
    $blocker.Dispose()
    $r = Wait-Run $run
    Check "4 locked turn file: waits, then LOCK OK once it can be read" (
        $still -and $r.Text -match "Waiting: the live app's turn file can't be read" -and $r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    if (-not $env:KANBAN_LOCK_TEST_QUICK) {
        Write-Host '      (the next check takes about 2 minutes; set KANBAN_LOCK_TEST_QUICK=1 to skip it)'
        $c = New-Case
        New-Item -ItemType Directory -Force -Path $c.Shared | Out-Null
        [IO.File]::WriteAllText((Join-Path $c.Shared 'turn-test.json'), 'not json')
        $r = Wait-Run (Start-Run $c) 200
        Check '4 turn file unreadable for 2 min: counted as absent with a warning, LOCK OK' (
            $r.Exit -eq 0 -and $r.Text -match 'LOCK OK' -and $r.Text -match 'WARNING: turn-test.json has been unreadable for 2 minutes' -and $r.Seconds -ge 120) $r
    }

    # --- 5. A turn file from an app that isn't running ------------------------------------
    $dead = 4000000
    while (Get-Process -Id $dead -ErrorAction SilentlyContinue) { $dead += 4 }
    $c = New-Case
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $dead $MyStart (New-Holder '105' '#105'))
    $r = Wait-Run (Start-Run $c)
    Check '5 turn whose appPid is not running: no wait' (Test-Clear $r) $r

    $c = New-Case
    $wrongStart = Format-Utc (Get-Process -Id $PID).StartTime.AddHours(-1)
    Write-Fixture $c 'turn-live.json' (New-Turn 'live' $PID $wrongStart (New-Holder '105' '#105'))
    $r = Wait-Run (Start-Run $c)
    Check '5 turn whose appStart is wrong (pid reused): no wait' (Test-Clear $r) $r

    # --- 6. An install turn that is ours --------------------------------------------------
    $c = New-Case
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart (New-Holder 'router' 'wave 3' 'install'))
    $r = Wait-Run (Start-Run $c -Owner 'router')
    Check '6 install turn for our own owner (router): no wait' (Test-Clear $r) $r

    # --- 7. -IgnoreTurn -------------------------------------------------------------------
    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-Turn 'live' $PID $MyStart (New-Holder '105' '#105'))
    $r = Wait-Run (Start-Run $c @('-IgnoreTurn') -Owner 'hugh')
    Check '7 -IgnoreTurn (Hugh) with a live foreign turn: no wait, prints a warning' ((Test-Clear $r) -and $r.Text -match 'WARNING: -IgnoreTurn') $r

    $c = New-Case
    $r = Wait-Run (Start-Run $c @('-IgnoreTurn') -Owner '105')
    Check '7 -IgnoreTurn from a card session: refused' ($r.Exit -ne 0 -and $r.Text -match 'for Hugh only' -and $r.Text -notmatch 'LOCK OK') $r

    # --- 8. hold.json ---------------------------------------------------------------------
    $c = New-Case
    Write-Fixture $c 'hold.json' ([ordered]@{ until = (Utc 10); by = 'hugh'; allowUntil = $null })
    $run = Start-Run $c @('-LockWaitMinutes', '0.02')
    $still = Wait-Waiting $run "Waiting: Hugh's hold" 3
    Write-Fixture $c 'hold.json' ([ordered]@{ until = $null; by = 'hugh'; allowUntil = $null })
    $r = Wait-Run $run
    Check "8 hold: waits past -LockWaitMinutes (0.02) without failing, says Hugh's hold" ($still -and $r.Text -match "Waiting: Hugh's hold until \d\d:\d\d") $r
    Check '8 hold: LOCK OK once the hold is cleared' ($r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    $c = New-Case
    Write-Fixture $c 'hold.json' ([ordered]@{ until = (Utc 10); by = 'hugh'; allowUntil = (Utc 10) })
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -1) })
    $r = Wait-Run (Start-Run $c @('-AssumeAppRunning', '-NoticeSeconds', '30'))
    Check '8 allowUntil in the future: no wait for the hold, the input or the notice' (Test-Clear $r) $r

    $c = New-Case
    Write-Fixture $c 'hold.json' ([ordered]@{ until = (Utc 30); by = 'hugh'; allowUntil = $null })
    (Get-Item (Join-Path $c.Shared 'hold.json')).LastWriteTimeUtc = [datetime]::UtcNow.AddHours(-3)
    $r = Wait-Run (Start-Run $c)
    Check '8 a hold written 3 h ago counts for 2 h at most: no wait' (Test-Clear $r) $r

    $c = New-Case
    New-Item -ItemType Directory -Force -Path $c.Shared | Out-Null
    [IO.File]::WriteAllText((Join-Path $c.Shared 'hold.json'), '{"until": ')
    [IO.File]::WriteAllText((Join-Path $c.Shared 'input-test.json'), 'garbage')
    $r = Wait-Run (Start-Run $c)
    Check '8 unreadable hold.json and input-test.json: counted as absent, no wait' (Test-Clear $r) $r

    # --- 9. input-test.json ---------------------------------------------------------------
    $c = New-Case
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -1) })
    $run = Start-Run $c
    $still = Wait-Waiting $run 'Waiting: Hugh used Test'
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -20) })
    $r = Wait-Run $run
    Check '9 input 1 min ago: waits, says when' ($still -and $r.Text -match 'Waiting: Hugh used Test at \d\d:\d\d') $r
    Check '9 input 1 min ago: LOCK OK once it is 20 min ago' ($r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    $c = New-Case
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -20) })
    $r = Wait-Run (Start-Run $c)
    Check '9 input 20 min ago: no wait' (Test-Clear $r) $r

    # --- 10. The restart notice -----------------------------------------------------------
    $c = New-Case
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -30) })
    $run = Start-Run $c @('-AssumeAppRunning', '-NoticeSeconds', '6')
    [void](Wait-Waiting $run 'Waiting: notice until' 0)
    $lock = Read-SharedJson (Get-WaitFile $c $run)
    $r = Wait-Run $run
    $restartAt = $null
    if ($lock) { $restartAt = (Get-UtcOrNull $lock.restartAt) }
    $startedAt = $null
    if ($lock) { $startedAt = (Get-UtcOrNull $lock.startedAt) }
    Check '10 notice (app running, input 30 min ago): restartAt is -NoticeSeconds after startedAt' (
        $restartAt -and $startedAt -and [math]::Abs(($restartAt - $startedAt).TotalSeconds - 6) -le 2) $r
    Check '10 notice: waits about -NoticeSeconds, says until when, then LOCK OK' (
        $r.Exit -eq 0 -and $r.Text -match 'LOCK OK' -and $r.Text -match 'Waiting: notice until \d\d:\d\d:\d\d' -and $r.Seconds -ge 6) $r

    $c = New-Case
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -30) })
    $r = Wait-Run (Start-Run $c @('-NoticeSeconds', '6'))
    Check '10 notice without -AssumeAppRunning (app counts as stopped): no wait' (Test-Clear $r) $r

    # --- 11. The DSH lock folder ----------------------------------------------------------
    $c = New-Case
    New-Item -ItemType Directory -Path $c.Dsh | Out-Null
    [IO.File]::WriteAllText((Join-Path $c.Dsh 'owner'), "105`r`n")
    $run = Start-Run $c
    $still = Wait-Waiting $run 'Waiting: the DSH lock folder'
    Remove-Item -LiteralPath $c.Dsh -Recurse -Force
    $r = Wait-Run $run
    Check '11 DSH folder of another owner (105): waits, says whose' ($still -and $r.Text -match 'Waiting: the DSH lock folder \(owner 105\)') $r
    # It removes the folder only while its owner file names this run, so "gone" also shows it wrote it.
    Check '11 then takes it with its owner, LOCK OK, and removes it at the end' (
        $r.Exit -eq 0 -and $r.Text -match 'LOCK OK' -and $r.Text -match 'Took the DSH lock folder .* \(owner tester\)' -and -not (Test-Path $c.Dsh)) $r

    $c = New-Case
    New-Item -ItemType Directory -Path $c.Dsh | Out-Null
    [IO.File]::WriteAllText((Join-Path $c.Dsh 'owner'), 'tester')
    $r = Wait-Run (Start-Run $c)
    Check '11 DSH folder of our own owner: used without a wait, and NOT removed at the end' (
        (Test-Clear $r) -and $r.Text -match 'Using the DSH lock folder' -and (Test-Path $c.Dsh) -and
        ([IO.File]::ReadAllText((Join-Path $c.Dsh 'owner')).Trim() -eq 'tester')) $r

    # --- 12. A killed holder frees install.lock -------------------------------------------
    $c = New-Case
    New-Item -ItemType Directory -Force -Path $c.Shared | Out-Null
    $holdCmd = "`$s = [IO.FileStream]::new('$($c.Lock)', [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, " +
        "[IO.FileShare]'Read, Delete', 4096, [IO.FileOptions]::DeleteOnClose); Start-Sleep -Seconds 90"
    $enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($holdCmd))
    $holder = Start-Process -FilePath 'powershell.exe' -ArgumentList "-NoProfile -EncodedCommand $enc" -NoNewWindow -PassThru
    $until = (Get-Date).AddSeconds(30)
    while (-not (Test-Path $c.Lock) -and (Get-Date) -lt $until) { Start-Sleep -Milliseconds 200 }
    $run = Start-Run $c @('-LockWaitMinutes', '5')
    $still = Wait-Waiting $run 'Waiting: install.lock'
    $holder.Kill()
    [void]$holder.WaitForExit(5000)
    $r = Wait-Run $run
    Check '12 a killed holder: Windows closes its handle, install.lock goes, the waiting run gets it' (
        $still -and $r.Exit -eq 0 -and $r.Text -match 'LOCK OK' -and -not (Test-Path $c.Lock)) $r

    # --- 13. F1: a run waits for a turn without install.lock ------------------------------
    $c = New-Case
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart (New-Holder 'router' 'wave 3' 'install'))
    $runA = Start-Run $c
    $stillA = Wait-Waiting $runA "Waiting: the test app's turn on Test: wave 3 (install)"
    $noLock = -not (Test-Path $c.Lock)
    $wait = Read-SharedJson (Get-WaitFile $c $runA)
    $rB = Wait-Run (Start-Run $c -Owner 'router')
    $aStill = -not $runA.Proc.HasExited
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart $null)
    $rA = Wait-Run $runA
    Check '13 F1: a run with no turn waits without install.lock, announced in install-wait-<pid>.json' (
        $stillA -and $noLock -and $wait -and $wait.phase -eq 'waiting' -and $wait.owner -eq 'tester' -and
        $wait.waitingFor -eq "the test app's turn on Test: wave 3 (install)") $rA
    Check "13 F1: meanwhile the turn holder's run (router) gets install.lock" ((Test-Clear $rB) -and $aStill) $rB
    Check '13 F1: the waiting run goes on once the turn ends, and leaves no wait file' (
        $rA.Exit -eq 0 -and $rA.Text -match 'LOCK OK' -and -not (Test-Path (Get-WaitFile $c $runA))) $rA

    # --- 14. F2: the notice starts when the other waits clear -----------------------------
    $c = New-Case
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -30) })
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart (New-Holder '105' '#105'))
    $run = Start-Run $c @('-AssumeAppRunning', '-NoticeSeconds', '5')
    $still = Wait-Waiting $run "Waiting: the test app's turn" 6
    $noNoticeYet = (Read-Shared $run.Out) -notmatch 'notice until'
    $cleared = [datetime]::UtcNow
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart $null)
    [void](Wait-Waiting $run 'Waiting: notice until' 0)
    $wait = Read-SharedJson (Get-WaitFile $c $run)
    $r = Wait-Run $run
    $afterClear = ([datetime]::UtcNow - $cleared).TotalSeconds
    $restartAt = $null
    if ($wait -and $wait.restartAt) { $restartAt = (Get-UtcOrNull $wait.restartAt) }
    Check '14 F2: no notice during the turn wait; restartAt is set from when the turn cleared' (
        $still -and $noNoticeYet -and $restartAt -and $restartAt -ge $cleared.AddSeconds(4)) $r
    Check '14 F2: waits the notice after the turn clears, then LOCK OK' (
        $r.Exit -eq 0 -and $r.Text -match 'LOCK OK' -and $afterClear -ge 5) $r

    # --- 15. F3: a DSH lock folder left by a run that has ended ----------------------------
    $dead = 4000000
    while (Get-Process -Id $dead -ErrorAction SilentlyContinue) { $dead += 4 }
    $c = New-Case
    New-Item -ItemType Directory -Path $c.Dsh | Out-Null
    [IO.File]::WriteAllText((Join-Path $c.Dsh 'owner'), "105`r`npid $dead $MyStart`r`n")
    $r = Wait-Run (Start-Run $c)
    $aside = @(Get-ChildItem -LiteralPath $c.Dir -Directory -Filter 'dsh-test-app.lock.released-*')
    Check '15 F3: a dead pid in the folder: renamed aside (kept), warned, taken, no wait' (
        (Test-Clear $r) -and $r.Text -match 'WARNING: The DSH lock folder belonged to a run that has ended' -and
        $aside.Count -eq 1 -and $aside[0].Name -match '\.released-\d{8}T\d{6}Z$' -and
        (Test-Path (Join-Path $aside[0].FullName 'owner')) -and
        $r.Text -match 'Took the DSH lock folder' -and -not (Test-Path $c.Dsh)) $r

    $c = New-Case
    New-Item -ItemType Directory -Path $c.Dsh | Out-Null
    [IO.File]::WriteAllText((Join-Path $c.Dsh 'owner'), "105`r`npid $PID $MyStart`r`n")
    $run = Start-Run $c
    $still = Wait-Waiting $run 'Waiting: the DSH lock folder (owner 105)'
    $notMoved = @(Get-ChildItem -LiteralPath $c.Dir -Directory -Filter '*.released-*').Count -eq 0
    Remove-Item -LiteralPath $c.Dsh -Recurse -Force
    $r = Wait-Run $run
    Check '15 F3: a live pid in the folder: waits, not moved; LOCK OK once it goes' (
        $still -and $notMoved -and $r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    # --- 16. Card #147: each slot has its own turn (turn files with "slots") ---------------
    $c = New-Case
    $h105 = New-Holder '105' '#105' 'tester' 105
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $h105 ([ordered]@{ test = (New-Slot 1 $null $h105) }))
    $runT = Start-Run $c
    $stillT = Wait-Waiting $runT "Waiting: the live app's turn on Test: #105 (tester)"
    $r2 = Wait-Run (Start-Run $c -Channel 'test2')
    $tStill = -not $runT.Proc.HasExited
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{}))
    $rT = Wait-Run $runT
    Check '16 a turn on Test only (turn-live.json with slots): a -Channel test run waits for it' (
        $stillT -and $rT.Text -match "Waiting: the live app's turn on Test: #105 \(tester\)" -and $rT.Exit -eq 0 -and $rT.Text -match 'LOCK OK') $rT
    Check '16 a turn on Test only: meanwhile a -Channel test2 run goes ahead' ((Test-Clear $r2) -and $tStill) $r2

    $c = New-Case
    $wave = New-Holder 'router' 'wave 3' 'install'
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2 $null $wave) }))
    $run = Start-Run $c -Channel 'test2'
    $still = Wait-Waiting $run "Waiting: the live app's turn on Test 2: wave 3 (install)"
    $waitText = Read-Shared (Get-WaitFile $c $run)
    $wait = Read-SharedJson (Get-WaitFile $c $run)
    $rOwn = Wait-Run (Start-Run $c -Owner 'router' -Channel 'test2')
    $aStill = -not $run.Proc.HasExited
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2) }))
    $r = Wait-Run $run
    Check "16 Test 2's turn held by someone else (router): a -Channel test2 run waits, says whose" (
        $still -and $r.Text -match "Waiting: the live app's turn on Test 2: wave 3 \(install\)") $r
    Check '16 the wait file of a -Channel test2 run carries "channel":"test2", and why it waits' (
        $waitText -match '"channel":"test2"' -and $wait -and $wait.channel -eq 'test2' -and $wait.phase -eq 'waiting' -and
        $wait.waitingFor -eq "the live app's turn on Test 2: wave 3 (install)") $r
    Check "16 meanwhile the holder's own owner (router) goes ahead on test2" ((Test-Clear $rOwn) -and $aStill) $rOwn
    Check '16 the waiting test2 run goes on once the turn is released' ($r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    # Every app's turn file counts, not just live's and this slot's own.
    $c = New-Case
    Write-Fixture $c 'turn-test3.json' (New-SlotTurn 'test3' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2 $null (New-Holder '7' '#7' 'tester' 7)) }))
    $run = Start-Run $c -Channel 'test2'
    $still = Wait-Waiting $run "Waiting: the test3 app's turn on Test 2: #7 (tester)"
    Write-Fixture $c 'turn-test3.json' (New-SlotTurn 'test3' $PID $MyStart $null ([ordered]@{}))
    $r = Wait-Run $run
    Check "16 turn-test3.json's turn on Test 2 also holds a -Channel test2 run, until it goes" (
        $still -and $r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    # --- 17. Card #147: an old turn file (no "slots") ------------------------------------
    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-Turn 'live' $PID $MyStart (New-Holder '105' '#105' 'tester' 105))
    $run = Start-Run $c
    $still = Wait-Waiting $run "Waiting: the live app's turn on Test: #105 (tester)"
    $r2 = Wait-Run (Start-Run $c -Channel 'test2')
    $aStill = -not $run.Proc.HasExited
    Write-Fixture $c 'turn-live.json' (New-Turn 'live' $PID $MyStart $null)
    $r = Wait-Run $run
    Check '17 an old turn-live.json (no slots) with a live holder: a -Channel test run waits for it' (
        $still -and $r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r
    Check '17 the same old turn-live.json: a -Channel test2 run does not wait' ((Test-Clear $r2) -and $aStill) $r2

    $c = New-Case
    Write-Fixture $c 'turn-test2.json' (New-Turn 'test2' $PID $MyStart (New-Holder '105' '#105' 'tester' 105))
    Write-Fixture $c 'turn-test.json' (New-Turn 'test' $PID $MyStart (New-Holder '106' '#106' 'tester' 106))
    $run = Start-Run $c -Channel 'test2'
    $still = Wait-Waiting $run "Waiting: the test2 app's turn on Test 2: #105 (tester)"
    Write-Fixture $c 'turn-test2.json' (New-Turn 'test2' $PID $MyStart $null)
    $r = Wait-Run $run
    Check "17 an old turn-test2.json holds test2 itself; an old turn-test.json doesn't" (
        $still -and $r.Exit -eq 0 -and $r.Text -match 'LOCK OK' -and $r.Text -notmatch '#106') $r

    # --- 18. Card #147: the owner wait (a slot given to a card) ---------------------------
    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2 140) }))
    $run = Start-Run $c -Owner '147' -Channel 'test2'
    $still = Wait-Waiting $run "Waiting: Test 2 holds card #140's build"
    $wait = Read-SharedJson (Get-WaitFile $c $run)
    $r140 = Wait-Run (Start-Run $c -Owner '140' -Channel 'test2')
    $rHugh = Wait-Run (Start-Run $c @('-IgnoreTurn') -Owner 'hugh' -Channel 'test2')
    $aStill = -not $run.Proc.HasExited
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{}))
    $r = Wait-Run $run
    Check '18 owner wait: Test 2 given to card #140 makes a run for 147 wait, naming card #140' (
        $still -and $r.Text -match "Waiting: Test 2 holds card #140's build \(the live board\)" -and
        $wait -and $wait.waitingFor -eq "Test 2 holds card #140's build (the live board)" -and $wait.owner -eq '147') $r
    Check '18 owner wait: meanwhile a run for card #140 itself goes ahead' ((Test-Clear $r140) -and $aStill) $r140
    Check '18 owner wait: -IgnoreTurn (Hugh) does not wait for the card' ((Test-Clear $rHugh) -and $rHugh.Text -match 'WARNING: -IgnoreTurn') $rHugh
    Check '18 owner wait: the run for 147 goes on once Test 2 is no longer given' ($r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r

    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2 140 (New-Holder 'router' 'wave 3' 'install')) }))
    $r = Wait-Run (Start-Run $c -Owner 'router' -Channel 'test2')
    Check "18 owner wait: an owner who holds Test 2's turn (router) goes ahead though card #140 has it" (Test-Clear $r) $r

    $dead = 4000000
    while (Get-Process -Id $dead -ErrorAction SilentlyContinue) { $dead += 4 }
    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $dead $MyStart $null ([ordered]@{ test2 = (New-Slot 2 140) }))
    $r = Wait-Run (Start-Run $c -Owner '147' -Channel 'test2')
    Check '18 owner wait: a card in the file of an app that is not running: no wait' (Test-Clear $r) $r

    # --- 19. Card #147: the wrong-slot stop ----------------------------------------------
    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2 147) }))
    $r = Wait-Run (Start-Run $c -Owner '147' -Channel 'test3')
    Check '19 wrong slot: card #147 has Test 2, so a -Channel test3 run for 147 stops at once, naming -Channel test2' (
        $r.Exit -ne 0 -and -not $r.TimedOut -and $r.Text -match "card #147's test app is Test 2: run this with -Channel test2" -and
        $r.Text -notmatch 'Waiting:' -and $r.Text -notmatch 'LOCK OK') $r
    Check '19 wrong slot: no install.lock, wait file or DSH lock folder left' (
        -not (Test-Path $c.Lock) -and -not (Test-Path $c.Dsh) -and @(Get-ChildItem -LiteralPath $c.Shared -Filter 'install-wait-*').Count -eq 0) $r
    $r2 = Wait-Run (Start-Run $c -Owner '147' -Channel 'test2')
    Check '19 wrong slot: the same card on its own slot (test2) goes ahead' (Test-Clear $r2) $r2

    # Given meanwhile: the run sees it at step 4, after install.lock, lets go and stops.
    $c = New-Case
    $held = Open-HeldLock $c.Lock ([ordered]@{ pid = $PID; owner = 'other'; label = 'held by the test'; phase = 'installing'; waitingFor = $null })
    $run = Start-Run $c -Owner '147' -Channel 'test3'
    $still = Wait-Waiting $run 'Waiting: install.lock'
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2 147) }))
    $held.Dispose()
    $r = Wait-Run $run
    Check '19 wrong slot after install.lock: Test 2 given to card #147 meanwhile stops its test3 run, which lets go' (
        $still -and $r.Exit -ne 0 -and -not $r.TimedOut -and $r.Text -match "card #147's test app is Test 2: run this with -Channel test2" -and
        $r.Text -notmatch 'LOCK OK' -and -not (Test-Path $c.Lock) -and -not (Test-Path $c.Dsh)) $r

    # --- 20. Card #147: Hugh's input and the notice per slot (input-<channel>.json) -------
    $c = New-Case
    Write-Fixture $c 'input-test3.json' ([ordered]@{ at = (Utc -1) })
    $run = Start-Run $c -Channel 'test3'
    $still = Wait-Waiting $run 'Waiting: Hugh used Test 3 at'
    $r2 = Wait-Run (Start-Run $c -Channel 'test2')
    $aStill = -not $run.Proc.HasExited
    Write-Fixture $c 'input-test3.json' ([ordered]@{ at = (Utc -20) })
    $r = Wait-Run $run
    Check '20 input-test3.json 1 min ago: a -Channel test3 run waits, says when, then LOCK OK' (
        $still -and $r.Text -match 'Waiting: Hugh used Test 3 at \d\d:\d\d' -and $r.Exit -eq 0 -and $r.Text -match 'LOCK OK') $r
    Check '20 input-test3.json 1 min ago: a -Channel test2 run does not wait for it' ((Test-Clear $r2) -and $aStill) $r2

    $c = New-Case
    Write-Fixture $c 'input-test.json' ([ordered]@{ at = (Utc -30) })
    $r = Wait-Run (Start-Run $c @('-AssumeAppRunning', '-NoticeSeconds', '30') -Channel 'test2')
    Check '20 notice: input-test.json 30 min ago gives a -Channel test2 run no notice' (Test-Clear $r) $r

    $c = New-Case
    Write-Fixture $c 'input-test2.json' ([ordered]@{ at = (Utc -30) })
    $r = Wait-Run (Start-Run $c @('-AssumeAppRunning', '-NoticeSeconds', '3') -Channel 'test2')
    Check '20 notice: input-test2.json 30 min ago gives a -Channel test2 run its notice' (
        $r.Exit -eq 0 -and $r.Text -match 'Waiting: notice until \d\d:\d\d:\d\d' -and $r.Text -match 'LOCK OK') $r

    # --- 21. Card #147: a card number is a card on the run's own board only ----------------
    # The test board is a copy of the live one: its #140 is another card than live's #140.
    $c = New-Case
    Write-Fixture $c 'turn-test.json' (New-SlotTurn 'test' $PID $MyStart $null ([ordered]@{ test3 = (New-Slot 3 140) }))
    $r = Wait-Run (Start-Run $c -Owner '140' -Channel 'test2')
    Check "21 the test board's #140 has Test 3: live #140's -Channel test2 run is not stopped" (Test-Clear $r) $r
    $run = Start-Run $c -Owner '140' -Channel 'test3'
    $still = Wait-Waiting $run "Waiting: Test 3 holds card #140's build (the test board)"
    Write-Fixture $c 'turn-test.json' (New-SlotTurn 'test' $PID $MyStart $null ([ordered]@{}))
    $r = Wait-Run $run
    Check "21 and live #140's -Channel test3 run waits for the test board's #140" ($still -and $r.Exit -eq 0) $r
    $c = New-Case
    Write-Fixture $c 'turn-test.json' (New-SlotTurn 'test' $PID $MyStart $null ([ordered]@{ test2 = (New-Slot 2 140) }))
    $r = Wait-Run (Start-Run $c -Owner '140' -Channel 'test3' -DataDir (Join-Path $c.Dir '.claude-kanban-dsh-test'))
    Check "21 a test-board session's own card #140 has Test 2: its -Channel test3 run stops, naming -Channel test2" (
        $r.Exit -ne 0 -and $r.Text -match '-Channel test2') $r

    # --- 22. Card #147: Hugh's own run goes ahead over a card's build, with a warning ----
    $c = New-Case
    Write-Fixture $c 'turn-live.json' (New-SlotTurn 'live' $PID $MyStart $null ([ordered]@{ test = (New-Slot 1 140) }))
    $r = Wait-Run (Start-Run $c -Owner 'hugh' -Channel 'test')
    Check "22 Hugh's own -Channel test run warns that Test holds card #140's build and goes ahead" (
        (Test-Clear $r) -and $r.Text -match "WARNING: Test holds card #140's build") $r
} finally {
    Remove-Item -LiteralPath $Base -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host "$($script:Passes) passed, $($script:Failures) failed."
if ($script:Failures -gt 0) { exit 1 }
exit 0
