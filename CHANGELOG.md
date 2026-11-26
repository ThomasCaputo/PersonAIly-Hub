# Changelog

All notable changes to PersonAIly Hub. Versions follow [Semantic Versioning](https://semver.org).
The section of each version is also used as the release notes shown by the in-app updater.

## [Unreleased]

## [1.3.2] — 2026-11-26

### Added
- **Fallback models:** a queue of cloud models to try, in order, when the chosen one returns an
  error (busy, too many requests, credit used up…). Settings → Fallback models holds the default
  queue; each chat (the "Fallback" menu above the messages) and each agent with a cloud model
  follows it or gets its own, starting as a copy of the default one. The reply starts again with
  the first model in the queue that answers, and stays with it until the end; any text the
  failing model had already written is discarded. A note above the reply says which model
  answered instead of which, and every message shows the name of its model. Model comparison
  keeps using exactly the models you picked.

### Security
- Updated the `cookie` library used by Tauri to 0.18.3, which fixes RUSTSEC-2026-0594 (private
  cookie nonces could repeat in processes that fork).

## [1.3.0] — 2026-11-26

### Added
- **MCP servers in chat:** the tools bar above the messages has an "MCP servers" menu to pick,
  for each conversation, the servers its model can use (also among the defaults for new
  conversations). The servers stay running between messages and close after 10 idle minutes.
  In chat every MCP tool asks for confirmation, even with "Allow everything in the folders",
  until you choose "Always allow" for that tool.
- **Skills:** ready-made instructions for certain tasks in the open `SKILL.md` format, created in
  the app or imported from a folder or a `.zip` (Settings → Skills). With the new "Skills" tool
  on, in chats and agents, the model sees only their names and descriptions and loads the
  instructions and files it needs when a request matches. Skills are included in backups.
- **Browser sign-in (OAuth) for remote MCP servers:** services such as Notion or Linear connect
  with "Sign in" instead of a pasted token. The app discovers the service's authorization
  server, registers itself when the service allows it, signs in with PKCE through your browser
  and renews the token by itself. Tokens are stored in Windows Credential Manager. Notion and
  Linear are in the list of known servers.
- **`/` commands from MCP prompts:** typing `/` in a chat lists the prompts offered by the
  conversation's MCP servers; after filling in their arguments, the prompt's text lands in the
  message box, to review and send. Servers that can suggest values for an argument
  (`completion/complete`) show them in a list under its box as you type.
- **Plugins:** packages with skills, MCP servers, agents and `/` commands, in the Claude Code
  plugin format, installed from a folder or a `.zip` (Settings → Plugins). Installing starts
  nothing; uninstalling removes everything the plugin created, secrets included. Plugin commands
  appear in the `/` menu of every conversation. Plugins are included in backups.

### Security
- **VS Code extension:** the `personailyHub.serverPath` and `personailyHub.dataDir` settings are
  now read only from your user settings, never from a project's `.vscode/settings.json`. A
  cloned repository could otherwise make the extension start a program of its own, or point it
  at a fake core.
- **"Always allow" on a delegation to another agent** now covers only further delegations. It
  used to raise the whole conversation to "Allow everything", which also ran terminal commands
  without asking.
- **Cloud provider addresses:** `http://` is accepted only when the host really is this
  computer (`localhost`, `127.0.0.1`, `[::1]`). Addresses such as `http://localhost.example.com`
  passed the old check, and the API key would have travelled unencrypted.
- **Plugins:** remote (HTTP) MCP servers of a plugin no longer take `${VARIABLES}` from your
  environment, which could have sent a token of yours to a site chosen by the plugin. Their
  missing values are listed after the installation, to fill in Settings. Local servers are
  unchanged.
- **Limits:** a message from an MCP server can be at most 16 MB, and a backup to restore at most
  20 GB once extracted, so a broken server or a crafted file cannot fill memory or disk.
- Confirmation requests are now numbered: a late answer to an old request can no longer approve
  a different call that happens to have the same identifier.

### Fixed
- The "MCP servers" menu above the messages now shows servers added in Settings without
  reloading the app.
- Opening the desktop app while the headless core started by VS Code was working no longer marks
  that core's agent sessions as interrupted, and a pending backup restore is no longer skipped:
  the headless core now closes before the app opens its data.
- A backup restore that fails halfway puts everything back as it was and is retried at the next
  start, instead of leaving the app with an empty archive.
- Creating a backup no longer blocks chats and agents while the file is compressed (about 1.5
  seconds every 100 MB of data): the archive is locked only for its copy.
- Replacing a plugin with a version that cannot be installed (for example, because one of its
  skills has the name of one of yours) now leaves the installed version in place.
- A browser sign-in (OAuth) is no longer cancelled by a stray request to its return address; the
  app keeps waiting for the right one.
- While a reply is arriving, a code block opened with four backticks is no longer split at an
  inner line of three.

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

[1.3.2]: https://github.com/ThomasCaputo/personaily-hub/compare/v1.2.1...v1.3.2
[1.3.0]: https://github.com/ThomasCaputo/personaily-hub/compare/v1.2.1...ed79438
[1.2.1]: https://github.com/ThomasCaputo/personaily-hub/compare/v1.0.0...v1.2.1
[1.0.0]: https://github.com/ThomasCaputo/personaily-hub/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/ThomasCaputo/personaily-hub/releases/tag/v0.1.0
