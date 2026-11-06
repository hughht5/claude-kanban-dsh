# Builds Claude Kanban DSH and installs it for this Windows user (no admin).
#
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1                 # live: stage an update
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Now            # live: install and restart now
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test   # the test copy, installed now
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test -Stage
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test -FreshCopy
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
# Running it twice is safe: the installer upgrades in place, kb.exe is copied over, and the
# PATH entry is only added when it is missing. It touches only your user account: no admin,
# no machine-wide settings. Written for Windows PowerShell 5.1.

param(
    [ValidateSet('live', 'test')]
    [string]$Channel = 'live',
    # Test only: replace the test board with a fresh copy of the live one (the old test board
    # is kept in the test data folder's backups).
    [switch]$FreshCopy,
    # Build and stage the installer for the app's "Update ready" banner; don't quit the app.
    [switch]$Stage,
    # Quit, install and restart straight away (the behaviour before -Stage existed).
    [switch]$Now,
    # Stage into another app's data folder (its update\ folder) instead of this app's own: for a
    # renamed app, whose new install doesn't exist yet. The old app shows "Update ready", and its
    # Restart now installs this build into this build's own folder and starts it there. Write
    # import-from.txt into the new data folder first, so the new app takes the old board.
    [string]$StageVia = ''
)

$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Test = $Channel -eq 'test'
if ($FreshCopy -and -not $Test) { throw '-FreshCopy is for -Channel test only; the live board is never replaced.' }
if ($Stage -and $Now) { throw 'Pass -Stage or -Now, not both.' }
if ($Stage) { $Mode = 'stage' } elseif ($Now) { $Mode = 'now' } elseif ($Test) { $Mode = 'now' } else { $Mode = 'stage' }
if ($StageVia -and $Mode -ne 'stage') { throw '-StageVia stages an update: drop -Now.' }
if ($StageVia -and -not (Test-Path (Join-Path $StageVia 'board.db'))) { throw "-StageVia: no board.db in $StageVia, so no app there to take the update." }
if ($FreshCopy -and $Mode -eq 'stage') { throw '-FreshCopy needs the test app stopped: use it with -Now.' }

# Outside AppData on purpose: Windows redirects AppData writes made by processes started from
# a packaged app (such as the Claude desktop app) into that app's own private copy, which
# launches from the Start menu or a shortcut never see.
if ($Test) {
    $InstallDir = Join-Path $env:USERPROFILE 'Programs\Claude Kanban DSH Test'
    $AppExe     = Join-Path $InstallDir 'claude-kanban-dsh-test.exe'   # mainBinaryName in tauri.test.conf.json
    $AppNames   = @('claude-kanban-dsh-test', 'Claude Kanban DSH Test')
    $QuitEvent  = 'Local\ClaudeKanbanDshQuitTest'
    $TargetDir  = Join-Path $Root 'target\test'
    $DataDir    = Join-Path $env:USERPROFILE '.claude-kanban-dsh-test'
    $Name       = 'Claude Kanban DSH (test)'
} else {
    $InstallDir = Join-Path $env:USERPROFILE 'Programs\Claude Kanban DSH'
    $AppExe     = Join-Path $InstallDir 'claude-kanban-dsh.exe'
    # The installed app and a development build from target\ use the same board, so stop either.
    $AppNames   = @('claude-kanban-dsh', 'Claude Kanban DSH')
    $QuitEvent  = 'Local\ClaudeKanbanDshQuit'
    $TargetDir  = Join-Path $Root 'target'
    $DataDir    = Join-Path $env:USERPROFILE '.claude-kanban-dsh'
    $Name       = 'Claude Kanban DSH'
}
$UpdateDir = Join-Path $DataDir 'update'
if ($StageVia) { $UpdateDir = Join-Path $StageVia 'update' }

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
    if ($Test) {
        $env:KANBAN_CHANNEL   = 'test'
        $env:CARGO_TARGET_DIR = $TargetDir
        cargo tauri build --config (Join-Path $Root 'src-tauri\tauri.test.conf.json')
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
