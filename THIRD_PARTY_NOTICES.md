# Third-party notices

LOTU Link is licensed under the MIT License (see `LICENSE`). The following
third-party components are distributed with the application. Their binaries
are shipped unmodified.

## scrcpy server 5.0.1

- Upstream: <https://github.com/Genymobile/scrcpy>
- License: Apache-2.0
- Full text: `src-tauri/licenses/scrcpy/LICENSE`
- Notice: `src-tauri/licenses/scrcpy/NOTICE`
- Pinned in `vendor.lock.json` and fetched by `scripts/fetch-vendor.mjs`.

## Android SDK Platform-Tools (ADB) 36.0.2

- Upstream: <https://developer.android.com/tools/releases/platform-tools>
- Downloaded from `https://dl.google.com/android/repository/platform-tools_r36.0.2-win.zip`
- License: Apache-2.0
- Full text: `src-tauri/licenses/adb/LICENSE`
- Files shipped: `adb.exe`, `AdbWinApi.dll`, `AdbWinUsbApi.dll` (unmodified).
- Pinned with SHA-256 in `vendor.lock.json`.

## Tauri 2 and plugins

- Tauri: <https://tauri.app/> — MIT / Apache-2.0
- `tauri-plugin-clipboard-manager`, `tauri-plugin-dialog`, `tauri-plugin-fs`:
  <https://github.com/tauri-apps/plugins-workspace> — MIT / Apache-2.0

## Frontend

- Svelte 5: <https://svelte.dev/> — MIT
- Vite: <https://vite.dev/> — MIT
- TypeScript: <https://www.typescriptlang.org/> — Apache-2.0
- Vitest: <https://vitest.dev/> — MIT

## Rust crates

Build dependencies (not all are shipped as separate files) include `tokio`,
`serde`, `thiserror`, `rand`, `futures-util`, `tokio-tungstenite`, `base64`,
`md-5`, `tauri` and `tauri-plugin-*`, under MIT and/or Apache-2.0 licenses.
See `Cargo.lock` for exact versions and each crate's package for its license.

## Runtime requirement

Microsoft WebView2 Runtime is not bundled. It is included in Windows 11 and in
recent Windows 10 installations; otherwise it can be installed from Microsoft.
