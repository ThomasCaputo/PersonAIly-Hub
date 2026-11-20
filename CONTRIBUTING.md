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
npm run check        # Rust formatting, lint and tests, translation check and UI tests
npm run build        # build the installer (target/release/bundle/nsis)
```

Other useful commands:

```
cargo build -p app-server                      # headless core (target/debug/app-server.exe)
npm run check:i18n -w desktop                  # list UI strings without an English translation
npm test -w desktop                            # UI tests (vitest), core translations included
node scripts/set-version.mjs 1.0.0             # change the version everywhere it is written
npm test -w personaily-hub-vscode              # VS Code extension tests and translation check
UPDATE_BINDINGS=1 cargo test -p app-server bindings   # regenerate packages/api-client/index.d.ts
npm run package -w personaily-hub-vscode       # build extensions/vscode/personaily-hub.vsix
powershell -File scripts/release.ps1           # release build: setup.exe + portable exe in release/
```

`scripts/release.ps1` strips local paths from the binaries and fails if the build still
contains the user name or profile path of the machine that built it. Official releases
are built by GitHub Actions (`.github/workflows/release.yml`) when a `v*` tag is pushed.

### Releasing a new version

1. `node scripts/set-version.mjs <version>` and write the `## [<version>]` section of
   `CHANGELOG.md`: it becomes the release notes and the text shown by the in-app updater.
2. `npm run check`, commit, then push the tag `v<version>`.
3. Review the draft release and publish it. Installed apps find the update through
   `latest.json`, attached to the release.

The in-app updater only accepts installers signed with the project's updater key. The
release build needs it in `TAURI_SIGNING_PRIVATE_KEY` (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`),
stored as GitHub secrets; the public half is `plugins.updater.pubkey` in
`apps/desktop/src-tauri/tauri.conf.json`. Losing the private key means installed apps can no
longer be updated automatically, so keep a copy somewhere safe.

Authenticode signing (the publisher name Windows shows, so SmartScreen does not warn) is
optional and done by `scripts/sign.ps1`: with SignPath (`SIGNPATH_API_TOKEN`,
`SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_POLICY_SLUG`, also as GitHub
secrets) or with a certificate installed on the machine (`WINDOWS_CERT_THUMBPRINT`).
`release.ps1` signs `app-server.exe`, the app and the installer *before* the updater
signature is computed; without those variables it builds unsigned and prints a warning.

## Repository layout

```
apps/desktop/        Tauri app (React + TypeScript UI, Rust shell)
crates/core/         Core: providers, local models, tools, agents, storage (`Core` methods in src/ops/)
crates/server/       Local API (also used by the app) and the headless app-server
extensions/vscode/   VS Code extension: chat, editor context, changes as diffs, agents
packages/api-client/ TypeScript types of the local API, generated from the Rust types
docs/                Requirements, architecture, roadmap and decision records (Italian)
scripts/             Release build and signing
```

## Translations

UI strings are written in Italian inside `t("…")`, with the English translation in
`apps/desktop/src/locales/en.ts`. `npm run check:i18n -w desktop` (also part of
`npm run check`) reports missing translations. The VS Code extension follows the same
scheme (`extensions/vscode/src/locales/en.ts`, plus `package.nls*.json` for its manifest). See
[ADR 0012](docs/adr/0012-lingue-interfaccia.md).

## Before opening a pull request

- Run `npm run check` and `npm run build:ui`: CI runs the same checks.
- Keep changes focused; explain the "why" in the description.
- By contributing you agree that your contribution is licensed under the
  [Apache License 2.0](LICENSE).
