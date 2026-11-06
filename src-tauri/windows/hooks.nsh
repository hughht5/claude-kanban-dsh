; NSIS hooks for the Claude Kanban DSH installer: put kb.exe (the board's command line) next to the
; app, and take it away on uninstall. `cargo tauri build` builds kb first (beforeBuildCommand).
; kb.exe is in the workspace's target folder. ${__FILEDIR__} is read here, at the top level,
; where it is this file's folder (src-tauri\windows); inside a macro it would be the script's.
; hooks.test.nsh defines it first for the test build, whose target folder is target\test.
!ifndef KANBAN_KB_EXE
  !define KANBAN_KB_EXE "${__FILEDIR__}\..\..\target\release\kb.exe"
!endif

!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  File "${KANBAN_KB_EXE}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\kb.exe"
!macroend
