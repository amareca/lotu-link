# LOTU Link

Aplicación de escritorio para ver y controlar un Android por USB, con capturas y
grabación de vídeo previstas. Windows es el primer objetivo; la arquitectura está
preparada para macOS y Linux.

## Requisitos

- Node.js y pnpm.
- Rust (toolchainstable de Tauri 2).
- WebView2 (incluida en Windows 11 y en las instalaciones recientes de Windows 10).
- ADB accesible en el `PATH` durante el desarrollo. El JAR de scrcpy está incluido
  en `src-tauri/resources/`.

## Desarrollo

```powershell
pnpm install
pnpm tauri dev
```

## Comprobaciones

```powershell
pnpm check
pnpm test
pnpm build
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Build de producción

```powershell
pnpm tauri build
```
