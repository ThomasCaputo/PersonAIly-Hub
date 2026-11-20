# Changelog

All notable changes to PersonAIly Hub. Versions follow [Semantic Versioning](https://semver.org).
The section of each version is also used as the release notes shown by the in-app updater.

## [1.0.0] — not released yet

### Added
- **Automatic updates.** The app checks GitHub for a new version at startup (can be turned off in
  Settings → Updates), downloads it, verifies its signature and installs it. The portable exe
  points to the releases page instead.
- **Data backup and restore** (Settings → Data backup): conversations, agents, settings and presets
  in one `.zip`. Before every database upgrade a copy is also kept automatically in `backups/`.
- **Diagnostic log** (`logs/core.log` in the data folder) and a "Copy diagnostic information"
  button for bug reports, with user name and profile path removed.
- **Conversation branches:** editing or regenerating no longer deletes anything; switch between
  versions with the ‹ › arrows. Export as JSON with all branches.
- **Model comparison:** two models side by side on the same message.
- **Hybrid GPU + CPU placement** for models that do not fit in VRAM, as an explicit advanced choice.
- **Searchable documents (local RAG)** with a local embedding model.
- **Restore points for agents:** undo everything an agent changed with one click.
- **Agents that delegate** parts of a task to other agents.
- **Inline code completion** in VS Code with a local code model (off by default).
- **Cloud spending limits**, per month and per conversation.
- **Command palette** (`Ctrl+K`) and drag-and-drop of files into the chat.
- The app keeps running in the notification area when the window is closed, and can unload the
  model after some idle minutes.

### Changed
- VRAM is read through the NVIDIA driver library instead of starting `nvidia-smi`.
- Faster streaming of long Markdown replies; smaller interface (KaTeX fonts in woff2 only).

### Fixed
- A lock file left by a crashed core no longer blocks the local API when Windows reuses its
  process id.
- Error messages shown in Italian while the interface was in English.

## [0.1.0] — 2026-11-14

First public version: local GGUF models through llama.cpp, Anthropic and OpenAI-compatible cloud
providers, chat with tools and permissions, agents with terminal and MCP, local OpenAI-compatible
API and VS Code extension, Italian and English interface.

[1.0.0]: https://github.com/ThomasCaputo/personaily-hub/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/ThomasCaputo/personaily-hub/releases/tag/v0.1.0
