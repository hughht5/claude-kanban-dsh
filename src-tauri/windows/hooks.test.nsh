; NSIS hooks for the test build (tauri.test.conf.json). The same hooks as hooks.nsh, but kb.exe
; comes from the test build's own target folder (CARGO_TARGET_DIR=target\test), so the test
; installer never picks up the live build's kb.exe.
!define KANBAN_KB_EXE "${__FILEDIR__}\..\..\target\test\release\kb.exe"
!include "${__FILEDIR__}\hooks.nsh"
