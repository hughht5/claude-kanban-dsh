# Builds Claude Kanban DSH and installs it for this Windows user (no admin).
#
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1                 # live: stage an update
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Now            # live: install and restart now
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test   # the test copy, installed now
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test -Stage
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test -FreshCopy
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test2  # test slot 2 (test3, ... alike)
#
# Two modes:
#
#   -Stage (the default for the live app) builds, then leaves the installer in the data
#   folder's update\ folder (%USERPROFILE%\.claude-kanban-dsh\update, or .claude-kanban-dsh-test\update)
#   with an update.json beside it (version, commit, built_at, installer, install_dir, app_exe).
#   The running app is not touched. Within 30 seconds it shows "Update ready" along the top of
#   the board; Restart now there suspends the sessions, installs silently and reopens the app
#   in the background, and the sessions that were mid-turn resume. A first install can't be
#   staged: use -Now.
#
#   -Now (the default for the test copy) is the old behaviour, steps 1 to 7 below: it quits the
#   running app, installs and starts it again straight away.
#
# What -Now does, in order:
#   1. builds kb and the app, and the NSIS installer (`cargo tauri build`);
#   2. quits a running copy of the same channel cleanly, so its sessions are suspended and
#      resumed later (the other channel's app is left alone);
#   3. runs the installer silently into %USERPROFILE%\Programs\Claude Kanban DSH (live) or
#      %USERPROFILE%\Programs\Claude Kanban DSH Test (test);
#   4. makes sure kb.exe is in that folder;
#   5. live: adds that folder to your user PATH, once. test: never, so `kb` stays the live one;
#   6. test: on the first install, or with -FreshCopy, copies the live board into the test
#      data folder (%USERPROFILE%\.claude-kanban-dsh-test) with `kb copy-board`, while the test app
#      is stopped. The live board is only read;
#   7. starts the app.
#
# The test channel is a full copy of the app beside the live one: its own build folder
# (target\test), install folder, data folder, port, quit event, single-instance lock and
# WebView profile, with a warning bar across the top of the board. Try a change there first;
# to swap it into the live app, run this script without -Channel from the same commit.
#
# More test slots run beside test the same way: -Channel test2, test3, ... (up to test10). Slot N
# has its own everything, named from N as crates\store\src\channel.rs names it: install folder
# Programs\Claude Kanban DSH Test N, exe claude-kanban-dsh-testN, data folder .claude-kanban-dsh-testN, build
# folder target\testN, identifier io.github.hughht5.claude-kanban-dsh.testN (so its own lock and WebView profile),
# quit event Local\ClaudeKanbanDshQuitTestN, and the DevTools port 9333 + N to launch it with. Its
# Tauri config and NSIS hooks are made from tauri.test.conf.json into target\testN at build time.
# -NamesOnly prints those names as JSON and stops (a store test checks them against channel.rs).
#
# Running it twice is safe: the installer upgrades in place, kb.exe is copied over, and the
# PATH entry is only added when it is missing. It touches only your user account: no admin,
# no machine-wide settings. Written for Windows PowerShell 5.1.
#
# Test installs take turns (-Channel test in mode now only; live and -Stage runs don't). After
# the build, before step 2, the script:
#   1. waits, saying why, while an app gave the turn to someone else (turn-live.json,
#      turn-test.json, while that app runs), Hugh holds the test app (hold.json) or used it in
#      the last 10 minutes (input-test.json), or the DSH lock folder
#      (%USERPROFILE%\.dsh-test-app.lock) is someone else's. It holds nothing meanwhile; it
#      announces itself in install-wait-<pid>.json in the shared folder
#      (%USERPROFILE%\.claude-kanban-dsh-shared\testapp);
#   2. then gives a restart notice: -NoticeSeconds when the test app is running and Hugh used
#      it in the last hour, else none (it starts again if a wait in 1 comes back).
#      hold.json's allowUntil ("let it restart") skips the hold, input and notice waits;
#   3. takes install.lock, open with delete-on-close, so a killed run frees it; while another
#      run holds it, it says who and tries again (the only wait that counts towards
#      -LockWaitMinutes);
#   4. looks at the turns once more: a turn given meanwhile sends it back to 1;
#   5. takes the DSH lock folder (or uses it when it is already this run's; a folder left by
#      a run that has ended is renamed aside), installs (steps 2 to 7), then lets go of both.
# The apps write those files and show install.lock's phase; the script only reads them and
# never calls an app. A run's owner is KANBAN_LOCK_OWNER, else KANBAN_CARD, else "hugh".
#
#   -For "wave 3"          the label the apps show for this run (default "#<card>", else the owner)
#   -IgnoreTurn            Hugh only: don't wait for the apps' turns (prints a warning)
#   -LockDir <path>        the shared folder; -DshLockDir <path> the DSH lock folder
#   -LockWaitMinutes 30    how long to wait for install.lock before giving up
#   -PollSeconds 5         how often to look again; -NoticeSeconds 120 the restart notice
#   -LockOnly              only the lock and the waits, then "LOCK OK" (scripts\test-install-lock.ps1);
#                          no build, no install. -AssumeAppRunning goes with it: count the test
#                          app as running for the notice (otherwise it counts as stopped).

param(
    # live, test, or a further test slot test2 to test10 (test1 is not a name: it is "test").
    [ValidatePattern('^(live|test|test[2-9]|test10)$')]
    [string]$Channel = 'live',
    # Test only: replace the test board with a fresh copy of the live one (the old test board
    # is kept in the test data folder's backups).
    [switch]$FreshCopy,
    # Print the names this channel uses as JSON and stop: nothing is built or touched.
    [switch]$NamesOnly,
    # Build and stage the installer for the app's "Update ready" banner; don't quit the app.
    [switch]$Stage,
    # Quit, install and restart straight away (the behaviour before -Stage existed).
    [switch]$Now,
    # Test installs take turns (see above). The label the apps show for this run.
    [string]$For,
    # Hugh only: don't wait for a turn an app gave to someone else.
    [switch]$IgnoreTurn,
    # The shared folder with install.lock and the apps' files, and the DSH lock folder.
    [string]$LockDir,
    [string]$DshLockDir,
    # How long to wait for another run's install.lock (fractions allowed), and how often to look.
    [double]$LockWaitMinutes = 30,
    [int]$PollSeconds = 5,
    # The restart notice when Hugh used the test app in the last hour.
    [int]$NoticeSeconds = 120,
    # Only the lock and the waits, then "LOCK OK"; with -AssumeAppRunning the test app counts as running.
    [switch]$LockOnly,
    [switch]$AssumeAppRunning
)

$ErrorActionPreference = 'Stop'

# --- Turns for test installs: the functions (called after the build) ---------------------

if (-not $LockDir)    { $LockDir    = Join-Path $env:USERPROFILE '.claude-kanban-dsh-shared\testapp' }
if (-not $DshLockDir) { $DshLockDir = Join-Path $env:USERPROFILE '.dsh-test-app.lock' }
$LockOwner = if ($env:KANBAN_LOCK_OWNER) { $env:KANBAN_LOCK_OWNER } elseif ($env:KANBAN_CARD) { $env:KANBAN_CARD } else { 'hugh' }
$LockLabel = if ($For) { $For } elseif ($env:KANBAN_CARD) { "#$env:KANBAN_CARD" } else { $LockOwner }
$script:LockStream  = $null   # install.lock, open with delete-on-close while this run holds it
$script:LockInfo    = $null   # what install.lock says
$script:DshMade     = $false  # this run made the DSH lock folder, so it removes it
$script:LockShown   = ''
$script:LockShownAt = [datetime]::MinValue
$script:TurnBadSince  = @{}   # channel -> since when its turn file can't be read
$script:TurnBadWarned = @{}

# UTC, RFC 3339, to the second.
function Format-LockTime([datetime]$Time) {
    $Time.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", [Globalization.CultureInfo]::InvariantCulture)
}

# A time from one of the files, as UTC; $null when it is missing or can't be read.
function ConvertTo-LockTime($Value) {
    if ($Value -is [datetime]) { return $Value.ToUniversalTime() }
    if ([string]::IsNullOrWhiteSpace([string]$Value)) { return $null }
    $t = [datetime]::MinValue
    $styles = [Globalization.DateTimeStyles]'AdjustToUniversal, AssumeUniversal'
    if ([datetime]::TryParse([string]$Value, [Globalization.CultureInfo]::InvariantCulture, $styles, [ref]$t)) { return $t }
    return $null
}

# One of the shared JSON files, opened with full sharing (the apps rename a .tmp file over
# it, which fails while someone has it open without delete sharing) and closed at once.
# State is 'ok' (Value is the JSON), 'missing', or 'bad' (locked, half-written or broken).
function Read-LockJsonState([string]$Path) {
    try {
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return [pscustomobject]@{ State = 'missing'; Value = $null } }
        $fs = [System.IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
        try {
            $ms = New-Object IO.MemoryStream
            $fs.CopyTo($ms)
        } finally { $fs.Dispose() }
        $text = [Text.Encoding]::UTF8.GetString($ms.ToArray()).TrimStart([char]0xFEFF)
        $value = $null
        if (-not [string]::IsNullOrWhiteSpace($text)) { $value = $text | ConvertFrom-Json }
        if ($null -eq $value) { return [pscustomobject]@{ State = 'bad'; Value = $null } }
        return [pscustomobject]@{ State = 'ok'; Value = $value }
    } catch [IO.FileNotFoundException], [IO.DirectoryNotFoundException] {
        return [pscustomobject]@{ State = 'missing'; Value = $null }
    } catch {
        return [pscustomobject]@{ State = 'bad'; Value = $null }
    }
}

# The same, where a file that can't be read counts as absent ($null) for this poll.
function Read-LockJson([string]$Path) { (Read-LockJsonState $Path).Value }

# install.lock is written through the handle this run holds (Set-Content can't open it).
# Emptied first, so a reader sees an empty (unreadable, so held) file, never a mix of two.
function Write-TestLock {
    $bytes = (New-Object Text.UTF8Encoding $false).GetBytes(($script:LockInfo | ConvertTo-Json -Compress))
    $s = $script:LockStream
    $s.SetLength(0)
    [void]$s.Seek(0, [IO.SeekOrigin]::Begin)
    $s.Write($bytes, 0, $bytes.Length)
    $s.Flush()
}

# install-wait-<pid>.json: this run waits (a plain file, written beside and renamed in).
function Write-WaitFile {
    $tmp = "$($script:WaitPath).tmp"
    try {
        [IO.File]::WriteAllText($tmp, ($script:LockInfo | ConvertTo-Json -Compress))
        if (Test-Path -LiteralPath $script:WaitPath) { [IO.File]::Replace($tmp, $script:WaitPath, [NullString]::Value) }
        else { [IO.File]::Move($tmp, $script:WaitPath) }
    } catch { Write-Warning "Could not write $($script:WaitPath) ($($_.Exception.Message))" }
}

function Remove-WaitFile {
    if ($script:WaitPath) {
        Remove-Item -LiteralPath $script:WaitPath, "$($script:WaitPath).tmp" -Force -ErrorAction SilentlyContinue
    }
}

function Get-LockHolderText([string]$Path) {
    $h = Read-LockJson $Path
    if (-not $h) { return 'install.lock, held by another run (its details cannot be read yet)' }
    $t = "install.lock, held by $($h.label) (owner $($h.owner), pid $($h.pid), $($h.phase)"
    if ($h.waitingFor) { $t += ", waiting for $($h.waitingFor)" }
    return "$t)"
}

# The DSH lock folder's owner file: line 1 the owner, line 2 "pid <pid> <pidStart>" (older
# writers leave out line 2).
function Get-DshOwnerLines {
    try { return @(([IO.File]::ReadAllText((Join-Path $DshLockDir 'owner'))) -split "`r?`n" | ForEach-Object { $_.Trim() }) }
    catch { return @() }
}

function Get-DshOwner {
    $lines = @(Get-DshOwnerLines)
    if ($lines.Count -gt 0) { return $lines[0] }
    return ''
}

# The app that wrote a turn file is still that app: the pid runs and started at appStart (2 s).
function Test-LockAppAlive($AppPid, $AppStart) {
    try {
        $start = ConvertTo-LockTime $AppStart
        if (-not $start) { return $false }
        $p = Get-Process -Id ([int]$AppPid) -ErrorAction Stop
        return ([math]::Abs(($p.StartTime.ToUniversalTime() - $start).TotalSeconds) -le 2)
    } catch { return $false }
}

# A DSH lock folder whose owner file names a process that has ended (or a reused pid) is
# dead: it is renamed aside (never deleted). $true when it was moved.
function Move-DeadDshLock {
    $lines = @(Get-DshOwnerLines)
    if ($lines.Count -lt 2 -or $lines[1] -notmatch '^pid\s+(\d+)\s+(\S+)$') { return $false }
    if (Test-LockAppAlive $Matches[1] $Matches[2]) { return $false }
    $aside = "$(Split-Path -Leaf $DshLockDir).released-$([datetime]::UtcNow.ToString("yyyyMMdd'T'HHmmss'Z'", [Globalization.CultureInfo]::InvariantCulture))"
    try {
        Rename-Item -LiteralPath $DshLockDir -NewName $aside -ErrorAction Stop
    } catch { return $false }
    Write-Warning "The DSH lock folder belonged to a run that has ended (owner $($lines[0]), $($lines[1])): moved it aside to $aside."
    return $true
}

# A turn an app gave to someone else (check a), or $null. The test app read is this run's own
# slot (turn-test.json, turn-test2.json, ...: each app writes turn-<its channel>.json, card #118).
function Get-TurnWait {
    if ($IgnoreTurn) { return $null }
    $now = [datetime]::UtcNow
    foreach ($ch in 'live', $Channel) {
        $read = Read-LockJsonState (Join-Path $LockDir "turn-$ch.json")
        if ($read.State -eq 'bad') {
            # Unreadable is not "nobody's turn": wait, for 2 minutes at most.
            if (-not $script:TurnBadSince.ContainsKey($ch)) { $script:TurnBadSince[$ch] = $now }
            if (($now - $script:TurnBadSince[$ch]).TotalMinutes -lt 2) { return "the $ch app's turn file can't be read" }
            if (-not $script:TurnBadWarned[$ch]) {
                Write-Warning "turn-$ch.json has been unreadable for 2 minutes: going on as if it were absent."
                $script:TurnBadWarned[$ch] = $true
            }
            continue
        }
        $script:TurnBadSince.Remove($ch)
        $script:TurnBadWarned.Remove($ch)
        $turn = $read.Value
        if (-not $turn -or -not $turn.holder) { continue }
        $h = $turn.holder
        $expires = ConvertTo-LockTime $h.expiresAt
        if (-not $expires -or $expires -le $now) { continue }
        if ([string]$h.owner -eq $LockOwner) { continue }
        if (-not (Test-LockAppAlive $turn.appPid $turn.appStart)) { continue }
        return "the $ch app's turn: $($h.label) ($($h.kind))"
    }
    return $null
}

# The first reason to wait (checks a to e; the notice is in Enter-TestLock), or $null.
# Sets $script:LockAllowed when hold.json's allowUntil says "let it restart".
function Get-TestLockWait {
    $now = [datetime]::UtcNow
    $turn = Get-TurnWait
    if ($turn) { return $turn }
    $holdPath = Join-Path $LockDir 'hold.json'
    $hold = Read-LockJson $holdPath
    $allow = $null
    if ($hold) { $allow = ConvertTo-LockTime $hold.allowUntil }
    $script:LockAllowed = [bool]($allow -and $allow -gt $now)
    if (-not $script:LockAllowed) {
        $until = $null
        if ($hold) { $until = ConvertTo-LockTime $hold.until }
        if ($until) {
            # A hold lasts at most 2 hours from when it was written.
            try {
                $cap = (Get-Item -LiteralPath $holdPath).LastWriteTimeUtc.AddHours(2)
                if ($until -gt $cap) { $until = $cap }
            } catch { }
            if ($until -gt $now) { return "Hugh's hold until $($until.ToLocalTime().ToString('HH:mm'))" }
        }
        $inp = Read-LockJson (Join-Path $LockDir 'input-test.json')
        $at = $null
        if ($inp) { $at = ConvertTo-LockTime $inp.at }
        if ($at -and ($now - $at).TotalMinutes -lt 10) { return "Hugh used the test app at $($at.ToLocalTime().ToString('HH:mm'))" }
    }
    if (Test-Path -LiteralPath $DshLockDir) {
        $o = Get-DshOwner
        if ($o -ne $LockOwner -and -not (Move-DeadDshLock)) {
            if (-not $o) { $o = 'not written yet' }
            return "the DSH lock folder (owner $o)"
        }
    }
    return $null
}

# The notice before a restart: -NoticeSeconds when the test app runs and Hugh used it in the
# last hour, else 0.
function Get-NoticeSeconds {
    if ($LockOnly) { $running = [bool]$AssumeAppRunning }
    else { $running = [bool](Get-Process -Name $AppNames -ErrorAction SilentlyContinue) }
    $inp = Read-LockJson (Join-Path $LockDir 'input-test.json')
    $at = $null
    if ($inp) { $at = ConvertTo-LockTime $inp.at }
    if ($running -and $at -and ([datetime]::UtcNow - $at).TotalMinutes -lt 60) { return $NoticeSeconds }
    return 0
}

# Says why the run waits when the reason changes, and again once a minute. -Record also
# writes it into the wait file's waitingFor when it changes.
function Show-TestLockWait([string]$Text, [switch]$Record) {
    $changed = $Text -ne $script:LockShown
    if ($Text -and ($changed -or ((Get-Date) - $script:LockShownAt).TotalSeconds -ge 60)) {
        Write-Host "  Waiting: $Text (looking again every $PollSeconds s)"
        $script:LockShownAt = Get-Date
    }
    if ($changed) {
        $script:LockShown = $Text
        if ($Record) {
            $script:LockInfo.waitingFor = $null
            if ($Text) { $script:LockInfo.waitingFor = $Text }
            Write-WaitFile
        }
    }
}

# The DSH lock folder: made with mkdir (no -Force), so only one taker wins. $true when this
# run may go on: it made the folder, or the folder is already this owner's (then it stays).
function Enter-DshLock {
    $made = $false
    foreach ($try in 1, 2) {
        try {
            New-Item -ItemType Directory -Path $DshLockDir -ErrorAction Stop | Out-Null
            $made = $true
            break
        } catch {
            if (-not (Test-Path -LiteralPath $DshLockDir)) { throw }
            if ((Get-DshOwner) -eq $LockOwner) {
                $script:DshMade = $false
                Write-Host "Using the DSH lock folder $DshLockDir (already owner $LockOwner; it stays afterwards)."
                return $true
            }
            if (-not (Move-DeadDshLock)) { return $false }
        }
    }
    if (-not $made) { return $false }
    $script:DshMade = $true
    [IO.File]::WriteAllText((Join-Path $DshLockDir 'owner'), "$LockOwner`r`npid $PID $($script:LockInfo.pidStart)`r`n")
    Write-Host "Took the DSH lock folder $DshLockDir (owner $LockOwner)."
    return $true
}

# Lets go of install.lock only (delete-on-close removes the file).
function Close-TestLockFile {
    if ($script:LockStream) {
        try { $script:LockStream.Dispose() } catch { }
        $script:LockStream = $null
    }
}

# Steps 1 to 5 of "Test installs take turns" at the top. It holds install.lock and the DSH
# lock folder when it returns.
function Enter-TestLock {
    $done = $false
    try {
        New-Item -ItemType Directory -Force -Path $LockDir | Out-Null
        $path = Join-Path $LockDir 'install.lock'
        $script:WaitPath = Join-Path $LockDir "install-wait-$PID.json"
        $pidStart = $null
        try { $pidStart = Format-LockTime (Get-Process -Id $PID).StartTime } catch { }
        $card = $null
        if ($env:KANBAN_CARD) { $card = [string]$env:KANBAN_CARD }
        $script:LockInfo = [ordered]@{
            pid        = $PID
            pidStart   = $pidStart
            owner      = $LockOwner
            label      = $LockLabel
            card       = $card
            commit     = $Commit
            worktree   = $Root
            startedAt  = Format-LockTime ([datetime]::UtcNow)
            phase      = 'waiting'
            waitingFor = $null
            restartAt  = $null
        }
        if ($IgnoreTurn) { Write-Warning '-IgnoreTurn: not waiting for a turn the apps gave to someone else.' }
        $lockWaited = [timespan]::Zero

        while ($true) {
            # 1 and 2. Wait without install.lock, announced in install-wait-<pid>.json, while a
            # turn, a hold, Hugh's input or the DSH lock folder stands in the way. The notice
            # starts when they are all clear, and again whenever one comes back and clears.
            $script:LockShown = ''
            $script:LockInfo.phase = 'waiting'
            $script:LockInfo.waitingFor = $null
            $script:LockInfo.restartAt = $null
            $restartAt = $null
            Write-WaitFile
            while ($true) {
                $reason = Get-TestLockWait
                $now = [datetime]::UtcNow
                if ($reason) {
                    $restartAt = $null
                } else {
                    if ($null -eq $restartAt) {
                        $notice = 0
                        if (-not $script:LockAllowed) { $notice = Get-NoticeSeconds }
                        $restartAt = $now.AddSeconds($notice)
                        $script:LockInfo.restartAt = Format-LockTime $restartAt
                        Write-WaitFile
                    }
                    if (-not $script:LockAllowed -and $now -lt $restartAt) {
                        $reason = "notice until $($restartAt.ToLocalTime().ToString('HH:mm:ss'))"
                    }
                }
                Show-TestLockWait $reason -Record
                if (-not $reason) { break }
                Start-Sleep -Seconds $PollSeconds
            }

            # 3. install.lock. Busy (a sharing violation) or delete-pending (access denied): try
            # again. Only this wait counts towards -LockWaitMinutes.
            Remove-WaitFile
            $script:LockShown = ''
            $tried = Get-Date
            while (-not $script:LockStream) {
                try {
                    $script:LockStream = [IO.FileStream]::new($path, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite,
                        [IO.FileShare]'Read, Delete', 4096, [IO.FileOptions]::DeleteOnClose)
                } catch {
                    $e = $_.Exception
                    while ($e.InnerException -and -not ($e -is [IO.IOException] -or $e -is [UnauthorizedAccessException])) { $e = $e.InnerException }
                    if (-not ($e -is [IO.IOException] -or $e -is [UnauthorizedAccessException])) { throw }
                    $holder = Get-LockHolderText $path
                    if (($lockWaited + ((Get-Date) - $tried)).TotalMinutes -ge $LockWaitMinutes) {
                        throw "Gave up after $LockWaitMinutes min waiting for $holder. Run this again when that install has finished."
                    }
                    Show-TestLockWait $holder
                    Start-Sleep -Seconds $PollSeconds
                }
            }
            $lockWaited += (Get-Date) - $tried
            $script:LockInfo.phase = 'installing'
            $script:LockInfo.waitingFor = $null
            Write-TestLock

            # 4. A turn given while this run waited for install.lock: let it go and wait again.
            $turn = Get-TurnWait
            if ($turn) {
                Write-Host "  Took install.lock, but now $turn; letting it go and waiting again."
                Close-TestLockFile
                continue
            }
            # 5. The DSH lock folder (taken meanwhile: let install.lock go and wait again).
            if (Enter-DshLock) { break }
            Write-Host '  Took install.lock, but the DSH lock folder was taken meanwhile; letting it go and waiting again.'
            Close-TestLockFile
        }
        Write-Host "Took $path for $LockLabel (owner $LockOwner). It is this run's turn."
        $done = $true
    } finally {
        if (-not $done) { Release-TestLock }
    }
}

# Removes the DSH lock folder if this run made it and it is still this owner's, removes the
# wait file, then closes install.lock (delete-on-close removes the file). Safe to call when
# nothing is held.
function Release-TestLock {
    if ($script:DshMade) {
        $script:DshMade = $false
        try {
            if ((Get-DshOwner) -eq $LockOwner) { Remove-Item -LiteralPath $DshLockDir -Recurse -Force }
        } catch { Write-Warning "Could not remove the DSH lock folder $DshLockDir ($($_.Exception.Message))" }
    }
    Remove-WaitFile
    Close-TestLockFile
}

$Root = Split-Path -Parent $PSScriptRoot
$Channel = $Channel.ToLowerInvariant()   # ValidatePattern ignores case; the build's check does not
# The slot: live 0, test 1, testN N. Every name below is derived from it.
if ($Channel -eq 'live') { $Slot = 0 } elseif ($Channel -eq 'test') { $Slot = 1 } else { $Slot = [int]$Channel.Substring(4) }
$Test = $Slot -gt 0
if ($FreshCopy -and -not $Test) { throw '-FreshCopy is for a test channel only; the live board is never replaced.' }
if ($Stage -and $Now) { throw 'Pass -Stage or -Now, not both.' }
if ($Stage) { $Mode = 'stage' } elseif ($Now) { $Mode = 'now' } elseif ($Test) { $Mode = 'now' } else { $Mode = 'stage' }
if ($StageVia -and $Mode -ne 'stage') { throw '-StageVia stages an update: drop -Now.' }
if ($StageVia -and -not (Test-Path (Join-Path $StageVia 'board.db'))) { throw "-StageVia: no board.db in $StageVia, so no app there to take the update." }
if ($FreshCopy -and $Mode -eq 'stage') { throw '-FreshCopy needs the test app stopped: use it with -Now.' }

# Outside AppData on purpose: Windows redirects AppData writes made by processes started from
# a packaged app (such as the Claude desktop app) into that app's own private copy, which
# launches from the Start menu or a shortcut never see.
if ($Slot -eq 1) {
    $InstallDir = Join-Path $env:USERPROFILE 'Programs\Claude Kanban DSH Test'
    $AppExe     = Join-Path $InstallDir 'claude-kanban-dsh-test.exe'   # mainBinaryName in tauri.test.conf.json
    $AppNames   = @('claude-kanban-dsh-test', 'Claude Kanban DSH Test')
    $QuitEvent  = 'Local\ClaudeKanbanDshQuitTest'
    $TargetDir  = Join-Path $Root 'target\test'
    $DataDir    = Join-Path $env:USERPROFILE '.claude-kanban-dsh-test'
    $Name       = 'Claude Kanban DSH (test)'
    $Identifier = 'io.github.hughht5.claude-kanban-dsh.test'
} elseif ($Test) {
    # Test slot N: the test names with N added (channel.rs derives the same).
    $InstallDir = Join-Path $env:USERPROFILE "Programs\Claude Kanban DSH Test $Slot"
    $AppExe     = Join-Path $InstallDir "claude-kanban-dsh-test$Slot.exe"   # mainBinaryName in the generated config
    $AppNames   = @("claude-kanban-dsh-test$Slot", "Claude Kanban DSH Test $Slot")
    $QuitEvent  = "Local\ClaudeKanbanDshQuitTest$Slot"
    $TargetDir  = Join-Path $Root "target\test$Slot"
    $DataDir    = Join-Path $env:USERPROFILE ".claude-kanban-dsh-test$Slot"
    $Name       = "Claude Kanban DSH (test $Slot)"
    $Identifier = "io.github.hughht5.claude-kanban-dsh.test$Slot"
} else {
    $InstallDir = Join-Path $env:USERPROFILE 'Programs\Claude Kanban DSH'
    $AppExe     = Join-Path $InstallDir 'claude-kanban-dsh.exe'
    # The installed app and a development build from target\ use the same board, so stop either.
    $AppNames   = @('claude-kanban-dsh', 'Claude Kanban DSH')
    $QuitEvent  = 'Local\ClaudeKanbanDshQuit'
    $TargetDir  = Join-Path $Root 'target'
    $DataDir    = Join-Path $env:USERPROFILE '.claude-kanban-dsh'
    $Name       = 'Claude Kanban DSH'
    $Identifier = 'io.github.hughht5.claude-kanban-dsh'
}
$UpdateDir = Join-Path $DataDir 'update'
$DevToolsPort = 9333 + $Slot

if ($NamesOnly) {
    [ordered]@{
        channel       = $Channel
        slot          = $Slot
        install_dir   = $InstallDir
        app_exe       = $AppExe
        quit_event    = $QuitEvent
        data_dir      = $DataDir
        name          = $Name
        identifier    = $Identifier
        devtools_port = $DevToolsPort
    } | ConvertTo-Json -Compress
    return
}

if ($Mode -eq 'stage' -and -not $StageVia -and -not (Test-Path $AppExe)) {
    throw "$Name isn't installed at $AppExe yet, so there is nothing to update. Run this with -Now for the first install."
}

# The build's identity, compiled into the app by src-tauri\build.rs and written into
# update.json, so the app can tell whether a staged build is newer than itself.
$Commit = (& git -C $Root rev-parse --short=12 HEAD 2>$null)
if (-not $Commit) { $Commit = 'unknown' }
if (& git -C $Root status --porcelain 2>$null) { $Commit = "$Commit-dirty" }
$BuiltAt = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
$Version = (Get-Content (Join-Path $Root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version

# Test installs take turns (the functions are at the top). -LockOnly stops here.
$LockGate = $Test -and $Mode -eq 'now'
if ($LockOnly -and -not $LockGate) { throw '-LockOnly is for -Channel test in mode now only.' }
if ($AssumeAppRunning -and -not $LockOnly) { throw '-AssumeAppRunning goes with -LockOnly only.' }
if ($IgnoreTurn -and $LockOwner -ne 'hugh') { throw "-IgnoreTurn is for Hugh only (this run's owner is $LockOwner)." }
if ($LockOnly) {
    try { Enter-TestLock; Write-Host 'LOCK OK' } finally { Release-TestLock }
    exit 0
}

# --- 1. Build ----------------------------------------------------------------------------

# cargo lives in ~\.cargo\bin; put it on this window's PATH if it isn't there already.
$CargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (($env:Path -split ';') -notcontains $CargoBin) { $env:Path = "$CargoBin;$env:Path" }

Write-Host "Building kb, the app and the installer for the $Channel channel (cargo tauri build)..."
$SavedChannel = $env:KANBAN_CHANNEL
$SavedTarget  = $env:CARGO_TARGET_DIR
$SavedCommit  = $env:KANBAN_GIT_COMMIT
$SavedBuilt   = $env:KANBAN_BUILT_AT
Push-Location $Root
try {
    $env:KANBAN_GIT_COMMIT = $Commit
    $env:KANBAN_BUILT_AT   = $BuiltAt
    # beforeBuildCommand in tauri.conf.json builds kb.exe first; the installer bundles it.
    # Both inherit these variables, so kb and the app are always the same channel.
    if ($Slot -eq 1) {
        $env:KANBAN_CHANNEL   = 'test'
        $env:CARGO_TARGET_DIR = $TargetDir
        cargo tauri build --config (Join-Path $Root 'src-tauri\tauri.test.conf.json')
    } elseif ($Test) {
        # Slot N: tauri.test.conf.json with the slot's names, and NSIS hooks that take kb.exe
        # from this slot's own target folder. Both are made in target\testN, so the checkout
        # stays clean (a dirty checkout would mark the build's commit "-dirty").
        $env:KANBAN_CHANNEL   = $Channel
        $env:CARGO_TARGET_DIR = $TargetDir
        New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
        $Utf8 = New-Object System.Text.UTF8Encoding($false)   # no BOM
        $Hooks = Join-Path $TargetDir 'hooks.nsh'
        $HookText = "; Made by scripts\install.ps1 for $Channel, as src-tauri\windows\hooks.test.nsh is for test.`r`n" +
            "!define KANBAN_KB_EXE `"$(Join-Path $TargetDir 'release\kb.exe')`"`r`n" +
            "!include `"$(Join-Path $Root 'src-tauri\windows\hooks.nsh')`"`r`n"
        [System.IO.File]::WriteAllText($Hooks, $HookText, $Utf8)
        $Conf = Get-Content (Join-Path $Root 'src-tauri\tauri.test.conf.json') -Raw | ConvertFrom-Json
        $Conf.productName          = "Claude Kanban DSH Test $Slot"
        $Conf.mainBinaryName       = "claude-kanban-dsh-test$Slot"
        $Conf.identifier           = $Identifier
        $Conf.app.windows[0].title = $Name
        # The slot's own DevTools port (9333 + N) in the window's browser args (card #101's
        # always-on port), so two test apps never share one.
        $Args0 = [string]$Conf.app.windows[0].additionalBrowserArgs
        $Args0 = $Args0 -replace '--remote-debugging-port=\d+', "--remote-debugging-port=$DevToolsPort"
        $Args0 = $Args0 -replace 'http://127\.0\.0\.1:\d+', "http://127.0.0.1:$DevToolsPort"
        $Conf.app.windows[0].additionalBrowserArgs = $Args0
        $Conf.bundle.windows.nsis.installerHooks = $Hooks
        # The icons are relative to src-tauri in the base config; give them in full here.
        $Conf.bundle.icon = @($Conf.bundle.icon | ForEach-Object { Join-Path $Root "src-tauri\$_" })
        $ConfPath = Join-Path $TargetDir "tauri.$Channel.conf.json"
        [System.IO.File]::WriteAllText($ConfPath, ($Conf | ConvertTo-Json -Depth 20), $Utf8)
        cargo tauri build --config $ConfPath
    } else {
        # A live build is never made with KANBAN_CHANNEL set, nor into another target folder.
        Remove-Item Env:KANBAN_CHANNEL -ErrorAction SilentlyContinue
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        cargo tauri build
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo tauri build failed (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
    $env:KANBAN_CHANNEL    = $SavedChannel
    $env:CARGO_TARGET_DIR  = $SavedTarget
    $env:KANBAN_GIT_COMMIT = $SavedCommit
    $env:KANBAN_BUILT_AT   = $SavedBuilt
}

$Setup = Get-ChildItem (Join-Path $TargetDir 'release\bundle\nsis') -Filter '*-setup.exe' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $Setup) { throw "No installer found in $TargetDir\release\bundle\nsis" }
$KbBuilt = Join-Path $TargetDir 'release\kb.exe'
Write-Host "Built $($Setup.FullName) ($Version, $Commit, $BuiltAt)"

# --- Stage: leave the installer for the app's "Update ready" banner, and stop here ---------

if ($Mode -eq 'stage') {
    New-Item -ItemType Directory -Force -Path $UpdateDir | Out-Null
    # A name of its own per build, so a copy never lands on an installer that is running.
    $Stamp  = (Get-Date).ToUniversalTime().ToString('yyyyMMddHHmmss')
    $Staged = Join-Path $UpdateDir "$($Setup.BaseName)-$Stamp.exe"
    Copy-Item $Setup.FullName $Staged -Force
    $Info = [ordered]@{
        version     = $Version
        commit      = $Commit
        built_at    = $BuiltAt
        installer   = $Staged
        install_dir = $InstallDir
        app_exe     = $AppExe
        channel     = $Channel
    }
    # Written beside it and moved into place, so the app never reads half a file.
    $Json = Join-Path $UpdateDir 'update.json'
    $Tmp  = "$Json.tmp"
    $Info | ConvertTo-Json | Set-Content -Path $Tmp -Encoding UTF8
    Move-Item -Path $Tmp -Destination $Json -Force
    # Older staged installers are build copies only; the newest one is all the app needs.
    Get-ChildItem $UpdateDir -Filter '*.exe' | Where-Object { $_.FullName -ne $Staged } |
        Remove-Item -Force -ErrorAction SilentlyContinue
    Write-Host "Staged $Staged"
    Write-Host "$Name shows 'Update ready' along the top of the board within 30 seconds (or at its next start)."
    Write-Host 'Restart now there installs it; nothing was quit or restarted. To install straight away instead, run this with -Now.'
    return
}

# --- Test installs take turns: install.lock, the waits, the DSH lock folder (see the top) --

if ($LockGate) { Enter-TestLock }

try {
# --- 2. Quit the running app cleanly ------------------------------------------------------

function Get-App { Get-Process -Name $AppNames -ErrorAction SilentlyContinue }

if (Get-App) {
    Write-Host "Quitting the running $Name app (its sessions are suspended)..."
    # The clean way: the app waits on this named event and quits as tray > Quit does,
    # stopping each session first. Each channel has its own event, so only this one quits.
    # Older builds don't have it; then we fall through.
    try {
        $quit = [System.Threading.EventWaitHandle]::OpenExisting($QuitEvent)
        [void]$quit.Set()
        $quit.Dispose()
    } catch {
        Write-Host '  (this build has no quit signal)'
    }
    # Stopping takes about two seconds per session, so allow a minute.
    Get-App | Wait-Process -Timeout 60 -ErrorAction SilentlyContinue

    if (Get-App) {
        # Next, an ordinary stop (no -Force). Sessions left running are marked orphaned
        # at the next start, and a mid-turn one is resumed then.
        Write-Host '  Still running; asking it to stop...'
        Get-App | Stop-Process -ErrorAction SilentlyContinue
        Get-App | Wait-Process -Timeout 15 -ErrorAction SilentlyContinue
    }
    if (Get-App) { throw 'The app is still running. Quit it from the tray, then run this again.' }
}

# --- 3. Install silently ------------------------------------------------------------------

Write-Host "Installing into $InstallDir..."
# /S is silent. /D sets the folder; NSIS wants it last and unquoted, even with spaces.
$p = Start-Process -FilePath $Setup.FullName -ArgumentList '/S', "/D=$InstallDir" -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "The installer failed (exit $($p.ExitCode))" }
if (-not (Test-Path $AppExe)) { throw "The installer finished, but $AppExe is missing" }

# --- 4. kb.exe next to the app ------------------------------------------------------------

# The installer puts it there; copy it as well in case an older installer didn't.
$KbInstalled = Join-Path $InstallDir 'kb.exe'
if (-not (Test-Path $KbInstalled)) {
    if (-not (Test-Path $KbBuilt)) { throw "kb.exe wasn't built at $KbBuilt" }
    Copy-Item $KbBuilt $KbInstalled -Force
}
Write-Host "kb.exe is at $KbInstalled"

if (-not $Test) {
    # --- 5. Put the folder on the user PATH, once (live only) -----------------------------

    # Read the user PATH from the registry as written (unexpanded), so entries like
    # %USERPROFILE% keep working, and write it back as the same kind of value. User scope only.
    $EnvKey  = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    $UserPath = [string]$EnvKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $Entries = @($UserPath -split ';' | Where-Object { $_ -ne '' })
    $Already = $Entries | Where-Object {
        [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -ieq $InstallDir.TrimEnd('\')
    }
    if ($Already) {
        Write-Host 'The install folder is already on your user PATH.'
    } else {
        $EnvKey.SetValue('Path', (($Entries + $InstallDir) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
        # Tell open programs (Explorer, new terminals) that the environment changed.
        Add-Type -Namespace ClaudeKanbanDsh -Name Env -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
'@
        $r = [UIntPtr]::Zero
        [void][ClaudeKanbanDsh.Env]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r)
        Write-Host 'Added the install folder to your user PATH (new terminals will see kb).'
    }
    $EnvKey.Close()
    if (($env:Path -split ';') -notcontains $InstallDir) { $env:Path = "$env:Path;$InstallDir" }
} else {
    Write-Host "The test folder is not added to PATH: run its kb as `"$KbInstalled`"."
    Write-Host "Launch it with DevTools port $DevToolsPort for tools\cdp.mjs (CDP_PORT=$DevToolsPort)."

    # --- 6. A copy of the live board for the test app (test only) -------------------------

    $TestBoard = Join-Path $DataDir 'board.db'
    if ($FreshCopy -or -not (Test-Path $TestBoard)) {
        # The test kb reads the live board (its default --from), read-only, with VACUUM INTO:
        # safe while the live app runs. -FreshCopy moves the old test board into backups.
        Write-Host "Copying the live board into $TestBoard..."
        $copyArgs = @('copy-board', '--to', $TestBoard)
        if ($FreshCopy) { $copyArgs += '--replace' }
        & $KbInstalled @copyArgs
        if ($LASTEXITCODE -ne 0) { throw "kb copy-board failed (exit $LASTEXITCODE)" }
    } else {
        Write-Host "Keeping the test board at $TestBoard (pass -FreshCopy for a new copy of the live one)."
    }
}

# --- 7. Start the app ---------------------------------------------------------------------

Write-Host "Starting $Name..."
# --background: it opens behind whatever is in front, without taking focus.
Start-Process -FilePath $AppExe -WorkingDirectory $InstallDir -ArgumentList '--background'
Write-Host 'Done.'
if ($LockGate) { $script:LockInfo.phase = 'started'; Write-TestLock }
} finally { Release-TestLock }
