# Changelog

All notable changes to PersonAIly Hub. Versions follow [Semantic Versioning](https://semver.org).
The section of each version is also used as the release notes shown by the in-app updater.

## [Unreleased]

## [1.2.1] — 2026-11-22

### Added
- **Model comparison keeps its conversations temporary:** the two columns no longer land in the
  chat history on their own. "Save as conversation" (or "Save both") keeps the ones you want;
  the others are deleted with "New comparison" or when the app closes. The comparison also
  survives switching to another page and back.
- **Reasoning on or off at load time:** models that can think before answering (Qwen3, Gemma 4,
  DeepSeek R1…) get a "Reasoning (thinking)" option in their card; turned off, they answer
  right away.

### Fixed
- **"Conversation roles must alternate" error with Gemma 3** (and other models whose chat
  template has no tools or wants strictly alternating roles): tool calls and results already in
  the conversation are sent to these models as text, and consecutive messages from the same role
  are merged. When tools are enabled, a notice explains that the model cannot use them.
- **Searchable documents:** a folder revoked from the models, or removed from the searchable
  documents, leaves the index right away; before, its passages stayed searchable until the next
  indexing.
- **Searchable documents:** a file the embedding model rejects is skipped instead of stopping
  the whole indexing, and passages are sized to the embedding model's context (512 tokens for
  `multilingual-e5`).
- **Searchable documents:** searches keep the index vectors in memory (normalized) instead of
  reading them all from the database at every question; the copy is dropped when the index
  changes or the embedding model goes idle.
- **Reasoning option:** detected from precise template markers only; for gpt-oss, which cannot
  turn reasoning off, "off" means the lowest reasoning effort.
- **Memory estimate of split models:** tensors are read from every part, not just the first.
- **Searchable documents:** the `query:`/`passage:` prefixes are chosen from the embedding
  model's file name only, so a folder name containing "e5" or "nomic" no longer changes them.
- **False "spilling into RAM" warning** with models whose input embeddings are large, such as
  Gemma 4 / Gemma 3n "E" models: llama.cpp always keeps those tables in RAM, and the warning
  mistook them for the driver spilling VRAM.
- **Memory estimate:** it now counts the embeddings that stay in RAM separately, and sizes the
  KV cache of sliding-window and shared-KV layers as llama.cpp does (Gemma 4 E4B at 32K context:
  about 6 GB of VRAM instead of 9.6).
- With a model already loaded, the check before reloading counts its memory as free, instead of
  saying the same model no longer fits.
- **Quantization label** taken from the file name when it has one: mixed quantizations such as
  Unsloth's `UD-Q6_K_XL` were shown as `Q8_0`.

### Security
- Updated the `url` dependency to 2.5.9 (RUSTSEC-2026-0592: stack overflow on nested `blob:`
  URLs).

## [1.0.0] — 2026-11-20

### Added
- **Guided first start:** detects the GPU, installs the right llama.cpp build and suggests a
  model that fits your memory, or connects a cloud provider.
- **MCP over HTTP** ("streamable HTTP") for remote servers, with headers such as
  `Authorization` stored in the Credential Manager; **MCP resources** the agents can list and
  read; a list of **well-known MCP servers** to add with one click.
- **Prompt caching with Anthropic**, with cache reads and writes counted in the cost estimate.
- **Agents are sturdier with small local models:** short paths relative to the working folder,
  and a per-step token limit after which the agent is asked to try again more briefly instead
  of stopping.
- **Accessibility:** skip link, screen-reader announcements for replies, errors and permission
  requests, labels on every field, keyboard renaming (`F2`), reduced motion, checked contrast in
  both themes.
- **User guide** in English and Italian (`docs/user-guide.md`, `docs/guida-utente.md`).
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
- 1.0 is for Windows only; Linux and macOS come later.
- VRAM is read through the NVIDIA driver library instead of starting `nvidia-smi`.
- Release builds can be signed with Authenticode (SignPath or a local certificate) once a
  certificate is available.
- Faster streaming of long Markdown replies; smaller interface (KaTeX fonts in woff2 only).

### Fixed
- A lock file left by a crashed core no longer blocks the local API when Windows reuses its
  process id.
- Error messages shown in Italian while the interface was in English.
- A path like `\src\file.txt` written by a model pointed at the root of the drive and was
  refused; it is now relative to the agent's working folder.

## [0.1.0] — 2026-11-14

First public version: local GGUF models through llama.cpp, Anthropic and OpenAI-compatible cloud
providers, chat with tools and permissions, agents with terminal and MCP, local OpenAI-compatible
API and VS Code extension, Italian and English interface.

[1.2.1]: https://github.com/ThomasCaputo/personaily-hub/compare/v1.0.0...v1.2.1
[1.0.0]: https://github.com/ThomasCaputo/personaily-hub/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/ThomasCaputo/personaily-hub/releases/tag/v0.1.0
