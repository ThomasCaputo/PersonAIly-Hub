# Contributing to PersonAIly Hub

Thanks for your interest! Bug reports and ideas go in
[Issues](https://github.com/ThomasCaputo/personaily-hub/issues); for larger changes,
open an issue first so we can agree on the approach.

## Building from source (Windows)

You need:

- **Rust** via [rustup](https://rustup.rs). The right version is pinned in
  `rust-toolchain.toml` and installed automatically.
- **Node.js 26** (the portable build is fine, as long as its folder is in `PATH`).
- **Visual Studio Build Tools** with the "Desktop development with C++" workload.
- **WebView2** (already part of Windows 11).

```
npm install          # once
npm run dev          # run the app in development mode
npm run check        # Rust formatting, lint and tests, plus the translation check
npm run build        # build the installer (target/release/bundle/nsis)
```

Other useful commands:

```
cargo build -p app-server                      # headless core (target/debug/app-server.exe)
npm run check:i18n -w desktop                  # list UI strings without an English translation
npm test -w personaily-hub-vscode              # VS Code extension tests
npm run package -w personaily-hub-vscode       # build extensions/vscode/personaily-hub.vsix
powershell -File scripts/release.ps1           # release build: setup.exe + portable exe in release/
```

`scripts/release.ps1` strips local paths from the binaries and fails if the build still
contains the user name or profile path of the machine that built it. Official releases
are built by GitHub Actions (`.github/workflows/release.yml`) when a `v*` tag is pushed.

## Repository layout

```
apps/desktop/        Tauri app (React + TypeScript UI, Rust shell)
crates/core/         Core: providers, local models, tools, agents, storage
crates/server/       Local API (also used by the app) and the headless app-server
extensions/vscode/   VS Code extension: chat, editor context, changes as diffs
docs/                Requirements, architecture, roadmap and decision records (Italian)
scripts/             Release build
```

## Translations

UI strings are written in Italian inside `t("…")`, with the English translation in
`apps/desktop/src/locales/en.ts`. `npm run check:i18n -w desktop` (also part of
`npm run check`) reports missing translations. See
[ADR 0012](docs/adr/0012-lingue-interfaccia.md).

## Before opening a pull request

- Run `npm run check` and `npm run build:ui`: CI runs the same checks.
- Keep changes focused; explain the "why" in the description.
- By contributing you agree that your contribution is licensed under the
  [Apache License 2.0](LICENSE).
