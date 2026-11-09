; NSIS hooks for the Claude Kanban DSH installer: put kb.exe (the board's command line) next to the
; app, and take it away on uninstall. `cargo tauri build` builds kb first (beforeBuildCommand).
; kb.exe is in the workspace's target folder. ${__FILEDIR__} is read here, at the top level,
; where it is this file's folder (src-tauri\windows); inside a macro it would be the script's.
; hooks.test.nsh defines it first for the test build, whose target folder is target\test.
!ifndef KANBAN_KB_EXE
  !define KANBAN_KB_EXE "${__FILEDIR__}\..\..\target\release\kb.exe"
!endif

; kb.exe is often running while the app installs: every Claude session in a checkout with the
; repo's .mcp.json keeps a `kb mcp` open. Windows won't overwrite or delete a running exe, and a
; silent install ignores the failed write, so the old kb.exe used to stay (card #126). Windows
; does let a running exe be renamed in its own folder, so the hook moves the old one aside to the
; first free kb.old-N.exe, writes the new one, then deletes the moved copy if nothing runs it.
; Copies still in use stay; the next install deletes them. They don't resolve as `kb`, so PATH
; and new sessions get the new kb.exe, and running ones keep the old copy until they end.
; If the new kb.exe doesn't land, the old one is moved back, so `kb` never goes missing.
; Whenever kb.exe is left as the old one, the installer exits 3 (card #134): the app itself is
; installed, so install.ps1 and Restart now's helper carry on, and say which processes hold it.

; This file is included at the top level of Tauri's installer.nsi, so it can declare these.
Var KanbanKbN
Var KanbanKbOld

!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  ; Copies left by earlier installs; those still in use stay (the delete just fails).
  Delete "$INSTDIR\kb.old-*.exe"
  StrCpy $KanbanKbOld ""
  IfFileExists "$INSTDIR\kb.exe" 0 kanban_kb_write
  StrCpy $KanbanKbN 1
  kanban_kb_try:
    IfFileExists "$INSTDIR\kb.old-$KanbanKbN.exe" kanban_kb_next 0
    ClearErrors
    Rename "$INSTDIR\kb.exe" "$INSTDIR\kb.old-$KanbanKbN.exe"
    IfErrors kanban_kb_next 0
    StrCpy $KanbanKbOld "$INSTDIR\kb.old-$KanbanKbN.exe"
    Goto kanban_kb_write
  kanban_kb_next:
    IntOp $KanbanKbN $KanbanKbN + 1
    IntCmp $KanbanKbN 99 kanban_kb_try kanban_kb_try 0
  ; Nothing could move it (99 copies in use, or a lock that refuses renames). Write over it
  ; anyway; if that fails too, the old kb.exe stays and the installer exits 3 (card #134).
  ClearErrors
  File /nonfatal "${KANBAN_KB_EXE}"
  IfErrors 0 kanban_kb_done
  DetailPrint "kb.exe is in use and couldn't be replaced; the old one stays"
  SetErrorLevel 3
  Goto kanban_kb_done
  kanban_kb_write:
  File "${KANBAN_KB_EXE}"
  StrCmp $KanbanKbOld "" kanban_kb_done 0
  IfFileExists "$INSTDIR\kb.exe" kanban_kb_landed 0
    ; The new one didn't land: put the old one back, so kb still runs, and exit 3.
    Rename "$KanbanKbOld" "$INSTDIR\kb.exe"
    SetErrorLevel 3
    Goto kanban_kb_done
  kanban_kb_landed:
    ; Not in use after all: the copy goes now. In use: the delete fails and it waits.
    Delete "$KanbanKbOld"
  kanban_kb_done:
!macroend

; Uninstall takes kb.exe and every kb.old-N.exe with it, then the emptied folder (card #137).
; This is POSTUNINSTALL on purpose, not PREUNINSTALL: Tauri's uninstall section runs PREUNINSTALL
; before its "is the app running?" check, and that check can abort (Hugh cancels "close the
; app?", or the app can't be stopped). kb.exe deleted there would leave the app installed with
; no `kb`. Here the app's own files are already gone. A silent update (install.ps1, Restart now)
; never runs the uninstaller; a setup.exe run by hand whose "uninstall first" choice is kept does,
; and its install then writes kb.exe afresh.
; A kb.exe or kb.old-N.exe that a session's `kb mcp` still runs can't be deleted, and a per-user
; uninstall can't schedule it for the next restart (that needs admin), so it and the folder stay
; until those sessions end; then they can be deleted by hand. The line below shows only in the
; uninstaller's own window, not in a silent uninstall.
!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\kb.exe"
  Delete "$INSTDIR\kb.old-*.exe"
  ; Tauri's own RMDir ran before this hook, while kb.exe was still there.
  RMDir "$INSTDIR"
  IfFileExists "$INSTDIR\*.*" 0 kanban_un_dir_gone
    DetailPrint "kb.exe or a kb.old-N.exe is in use by a running session, so $INSTDIR stays until it ends"
  kanban_un_dir_gone:
!macroend
