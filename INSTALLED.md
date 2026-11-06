# Installed for the build

Everything installed on this machine to build the app, with where it came from and how to remove it. An entry is added at the moment of install, never afterwards. Only official sources are used, and no system security setting is changed.

**Status (3 Nov 2026):** Hugh said "install the Rust and Tauri toolchain" in the build session, and the installs below were made the same day.

## Already on the machine (not installed by this work)

| Tool | Version | Note |
|---|---|---|
| Node.js | 24.19.0 | on `PATH` |
| WebView2 Runtime | part of Windows 11 | checked again in Phase 1 |
| Git, `gh` | existing | |

## Planned for Phase 1

| Tool | Source | Uninstall |
|---|---|---|
| rustup and the stable Rust toolchain (`x86_64-pc-windows-msvc`) | `rustup-init.exe` from https://rustup.rs | `rustup self uninstall` (removes `~\.rustup` and `~\.cargo`) |
| Visual Studio Build Tools 2022, "Desktop development with C++" workload (MSVC and the Windows SDK) | https://visualstudio.microsoft.com/downloads/ (Build Tools for Visual Studio). It may need an admin install, which Hugh would run himself. | Visual Studio Installer, then Uninstall |
| Tauri CLI (`cargo install tauri-cli --locked`) | crates.io | `cargo uninstall tauri-cli` |
| Crates in `Cargo.lock`, npm packages in `ui-deps/package-lock.json` | crates.io, npmjs.com, pinned by the lockfiles | delete `target\` and `ui-deps\node_modules\` |

## Installed

| Date | Tool | Version | Source | Uninstall |
|---|---|---|---|---|
| 3 Nov 2026 | Visual Studio Build Tools 2022, workload `Microsoft.VisualStudio.Workload.VCTools` with recommended components (MSVC, Windows SDK), in `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` | 17.14.44 | `winget install --id Microsoft.VisualStudio.2022.BuildTools` (the winget community source, which downloads Microsoft's own `vs_BuildTools.exe` from download.visualstudio.microsoft.com and checks its hash). This accepted the Build Tools licence and the winget source agreement. | `winget uninstall --id Microsoft.VisualStudio.2022.BuildTools`, or the Visual Studio Installer, then Uninstall |
| 3 Nov 2026 | rustup, with the stable toolchain `x86_64-pc-windows-msvc` (default profile: rustc, cargo, clippy, rustfmt, rust-docs). rustup added `%USERPROFILE%\.cargo\bin` to the user `PATH`. | rustup-init from static.rust-lang.org, SHA-256 `38027d51…c7db` matching the published `.sha256`; rustc 1.99.1, cargo 1.99.1 | https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe | `rustup self uninstall` (removes `~\.rustup`, `~\.cargo` and the `PATH` entry) |
| 3 Nov 2026 | Tauri CLI (`cargo-tauri`) | 2.12.4 | `cargo install tauri-cli --version "^2" --locked`, from crates.io | `cargo uninstall tauri-cli` |
| 3 Nov 2026 | Crates for the store and `kb` (rusqlite with bundled SQLite, serde, serde_json, regex, thiserror, chrono, tempfile), pinned in `Cargo.lock` | per `Cargo.lock` | crates.io | delete `target\`; the download cache goes with `rustup self uninstall` |
| 6 Nov 2026 | rustup targets `aarch64-apple-darwin` and `x86_64-apple-darwin` (the standard library for macOS only, so `cargo check -p kanban-unix --target aarch64-apple-darwin` can check card #117's Mac code here; no Apple SDK, nothing links) | the stable toolchain's | `rustup target add`, from static.rust-lang.org | `rustup target remove aarch64-apple-darwin x86_64-apple-darwin` |
