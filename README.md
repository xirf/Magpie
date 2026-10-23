# Mornye

A general-purpose Rust personal assistant for conversation, learning, planning, writing, and practical help on a small always-on computer. Hosted inference, SQLite memory, a persistent personality, local code indexing, and direct host tools with automatic approval by default.

Source-grounded revision notes and remaining gaps: [upstream audit](docs/upstream-audit.md).

## What works

- CLI chat and an owner-authorized Discord gateway service. No inbound port is required.
- DeepSeek chat/tool calls; a configurable Chat Completions-compatible endpoint.
- SQLite conversations and action evidence, bounded model context, restart recovery, and a single-process state lock.
- Personal memory with full-text search, source notes, inspection, and forgetting.
- A user-editable `SOUL.md`, created in the state directory on first launch.
- Learned skill candidates, explicit promotion, retrieval, and retirement.
- Rust/TypeScript/TSX syntax indexing, source hashes, and stale-result detection.
- Host file reads, directory listing, file changes/deletion, and shell commands with configurable approval.
- Real command exit codes and bounded stdout/stderr. Coding tests, SSH operations, and Wrangler deployments can run through the shell tool.
- One-time scheduled tasks while the Discord service is running.

This is v0.1. It is not a feature-complete Hermes/OpenClaw port. No local inference, Docker, vector database server, or browser service is required.

## Start on your development machine

Install a current stable Rust toolchain and its platform C compiler (SQLite and tree-sitter are bundled native dependencies).

```powershell
Copy-Item mornye.example.toml mornye.toml
$env:DEEPSEEK_API_KEY = "your-api-key"
cargo run -- doctor
cargo run -- chat
```

Linux:

```sh
cp mornye.example.toml mornye.toml
export DEEPSEEK_API_KEY='your-api-key'
cargo run -- doctor
cargo run -- chat
```

Use your provider's current model identifier in `mornye.toml`. No credentials are bundled or stored in that file. The runtime fails clearly when the key is missing. Provider errors are not retried automatically, avoiding unexpected repeated charges; `/retry` is explicit.

`mornye ask "your message"` runs a single turn and preserves the CLI conversation. `mornye ask "/pending"` inspects a pending review. Only one process may use a state directory at once, including CLI versus Discord.

## Tool approval

`auto_approve = true` is the default, including for existing config files that omit it. File writes, deletion, shell commands, and scheduling execute without confirmation. The runtime records action arguments and results as before. Previously pending actions resume automatically on the next message or `/retry` after switching to this mode.

Set `auto_approve = false` in `mornye.toml` and restart to require manual review for each file change, deletion, shell command, or scheduled task. In manual mode, Mornye displays the exact arguments and an action ID. Use `/approve EXACT-ID` or `/deny EXACT-ID`. Approval is single use, scoped to the conversation owner, and expires after 24 hours. Plain `yes`, model output, and retrieved text cannot approve actions or change the configured mode.

Tools run directly with the host user's access. Both modes preserve input validation, file hash checks, command timeouts, bounded output, and action evidence. Interrupted actions are recorded as uncertain after restart and are never replayed automatically. Use `/status` to inspect evidence; a command's exit code is not a deployment health check. Skill promotion and memory management remain explicit user commands.

## Discord / continuous operation

1. Create a bot application in the Discord Developer Portal.
2. Set `DISCORD_BOT_TOKEN` in the environment.
3. Enable Developer Mode in Discord; copy your user ID into `discord_user_id`.
4. Run `mornye serve` and DM the bot. For a server channel, set `discord_channel_id`, enable Message Content Intent, and invite the bot with View Channel and Send Messages permissions.

The owner ID is mandatory. Other users, bots, webhooks, and other channels are ignored. Without a channel ID, only DMs are accepted. With a channel ID, only that exact channel is accepted. Message commands such as `/approve UUID` are ordinary message text, not registered Discord application slash commands. Attachments, voice, and message edits are not processed.

Mornye receives explicit Discord context: conversational replies in the user's language, her calm and warm persona, and short paragraphs or useful lists instead of Markdown tables. Existing `SOUL.md` and `MORNYE_PROFILE.md` edits are preserved. Coding remains available when requested.

While a request runs, the bot sends a short status message, refreshes Discord's typing indicator about every four seconds, and edits that status when activity changes (for example, thinking or running a command). Status text comes from runtime events and excludes tool arguments and private reasoning. The first final reply chunk replaces the status; any remaining chunks are sent afterward. The status message ID is persisted so crash recovery can replace it with an interruption notice. If the status was deleted, final delivery falls back to a new message. Status and typing requests are best effort with short timeouts; final replies remain in the durable delivery queue until acknowledged. Blocking host work can delay refreshes, and a timeout after Discord accepts a status can leave an orphan status message.

Design rationale, reference mapping, and validation limits: [conversational harness](docs/conversational-harness.md).

Serenity manages gateway heartbeat, reconnection and rate limits on a dedicated runtime. Its guild/member cache and command framework are disabled. Requests are persisted before model execution, deduplicated by Discord message ID, and bounded to 64 waiting/running requests. A restart preserves queued requests; interrupted running requests are reported and never replayed automatically. Replies are committed together with request completion and delivered in UTF-16-bounded chunks with mentions disabled. A crash after Discord accepts a reply but before local acknowledgement can duplicate that reply chunk. Messages sent while fully offline are not backfilled. This is not exactly-once delivery.

Conversations and approvals are scoped to owner plus channel. The state directory is bound to its first Discord owner/channel configuration. The agent processes one turn at a time; the gateway continues receiving while it works. `/cancel` is processed when the current model/tool call returns, not as an immediate interrupt. An undeliverable reply blocks further agent execution until delivery succeeds, while the bounded ingress queue remains available.

One-time schedules are persisted and dispatch when their Discord conversation is idle. A stable queue ID prevents duplicate dispatch across restart. `dispatched` means queued for the agent, not successful completion. CLI schedules do not run in Discord. `/schedules` and `/unschedule ID` inspect or cancel schedules.

## Memory, personality, and learning

Mornye initializes a Wuthering Waves inspired character reference at `.mornye/MORNYE_PROFILE.md` and includes it as fictional tone and lore context each turn. It never claims to be the literal game character. Existing profile edits are preserved across restarts. Edit `.mornye/SOUL.md` for additional personal preferences (up to 8 KiB); that file is also loaded each model turn. The original, non-verbatim profile and source links are in [docs/mornye-character-profile.md](docs/mornye-character-profile.md). Personal facts retain the actual originating user-message ID alongside a separately labeled model source claim. Exact repeated facts are deduplicated per conversation, and each saved fact carries its originating user message ID. Facts start unverified. Use `/rate ID useful` or `/rate ID wrong` to record feedback; a wrong fact leaves retrieval. `/correct ID corrected fact` supersedes it, stores the replacement as user confirmed, and retains a correction event. `/memories` shows active, disputed, and superseded entries; `/forget ID` erases one fact. Retrieval is scoped SQLite full-text search with user feedback attached; semantic embeddings and paraphrase recall are not implemented.

After useful work, the model can propose a reusable procedure with a model evidence claim and a snapshot of actual recent action records. These records are not proof of skill success. `/skills` shows candidates and active entries. Read the procedure and evidence, then `/promote ID`; `/retire ID` disables it. Skills remain data and never bypass approval. Candidate creation and reuse work; automatic evaluation, measured skill scoring, model fine-tuning, and self-updating executables are not implemented.

Ask Mornye to index the project and search for a symbol. The code index extracts definitions, syntactic calls, and imports for `.rs`, `.ts`, and `.tsx`. It skips common build/vendor directories, limits each scan to 500 source files and 5,000 directory entries, and caps files at 256 KiB. Repeated indexing reparses changed files only. Search checks hashes and marks changed/deleted source as stale. It is not full type resolution or a drop-in Graphify implementation; macro expansion, inferred cross-file links, and embeddings are not included.

## Coding and deployment

Example request:

> Inspect this project, fix the bug, show me the changes, run its tests, then prepare a staging deployment. Ask before each command or file change.

Mornye can inspect/index files automatically and propose actual edits. After approval it can run the project's tests and inspect stdout, stderr, and exit status. `/status` keeps the last ten action records; the full ledger is in SQLite. Tell it the repository and acceptance criteria. Successful tests must apply to the final source revision; the generic shell tool does not independently enforce artifact provenance.

For remote server deployment, configure SSH credentials on the execution host, preserve host-key checking, and review the exact SSH command. For Workers, install/configure Wrangler on the host or remote runner and review commands for tests, staging deploy, HTTP verification, production deploy, and rollback separately. Nothing is deployed during setup. No remote account or deployment credential is assumed. This version offers shell execution with configurable approval, not a specialized deployment controller with automatic health gates or rollback.

## B860H deployment

The user's box has been verified as **aarch64 / 64-bit Armbian on Ubuntu 24.04**. The earlier ARM64 baseline and its 13 tests ran successfully there. The Discord queue and adapter test binary passed all 15 initial tests on the device (3.52 seconds). The memory and migration revisions passed 17 ARM64 tests on the B860H in 4.61 seconds. The earlier idle CLI measured about **4 MiB RSS**; that number does not measure this Discord build or peak workloads. See [device validation](docs/validation-armbian.md) for exact results and caveats. Build on a stronger computer or CI and copy the binary; do not make the 2 GB box compile the full dependency tree for routine updates.

- 64-bit Linux userspace commonly needs `aarch64-unknown-linux-gnu`.
- 32-bit hard-float Linux userspace may need `armv7-unknown-linux-gnueabihf`.
- Android/Termux needs its own compatible target/build setup; the systemd template is for Linux.

Cross-compilation requires the matching Rust target plus linker/C toolchain; `rustup target add` alone is insufficient. The tested build used `cargo zigbuild --release --locked --target aarch64-unknown-linux-gnu.2.31`. Build against a compatible libc baseline.

The first tested CLI baseline remains at `/home/rustfs_user/rustfs_data/mornye`. The source-grounded Discord/persona build is deployed from `/home/rustfs_user/rustfs_data/mornye/workspace-discord-persona`, with binary `bin/mornye-discord-persona-20261017`. On 2026-10-18, `magpie-sdcard.service` was stopped and disabled before its Discord token was provisioned to Mornye. The Mornye service is enabled and running as `mornye-discord-persona.service`; it loads its Discord and DeepSeek credentials from the root-only `mornye/secrets.env` on the microSD. It uses the existing Mornye state and preserves the user-editable `SOUL.md`. With no channel ID configured, it accepts owner direct messages only. See [device validation](docs/validation-armbian.md) for the backup and live status.

`deploy/mornye.service` is an example for `/opt/mornye` and a `mornye` host account. Adjust paths and permissions, place environment secrets in `/etc/mornye.env` readable only by the service account/root, and install/enable the unit when ready. It imposes a 512 MiB service/process-group memory limit and configures restart, but deliberately adds no filesystem/network sandbox. Running as root is unnecessary; tools receive whatever permissions the service user has.

Keep state on durable storage, limit unrelated services, and back up while Mornye is stopped (copy the complete state directory, including SQLite WAL files if present). The database contains personal conversations and tool output in plaintext. Context/output memory is bounded; long-term history is retained. Periodically inspect disk usage. Do not put secrets into prompts or printed command output.

## Commands and checks

Use `/help` for controls. Useful commands: `/pending`, `/approve ID`, `/deny ID`, `/status`, `/retry`, `/cancel`, `/new`, `/memories`, `/forget ID`, `/rate ID useful|wrong`, `/correct ID replacement`, `/skills`, `/promote ID`, `/retire ID`, `/schedules`, `/unschedule ID`.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
```

Tests use a loopback mock provider and temporary directories. They do not use paid inference or deploy remotely.

## CI builds and tagged releases

GitHub Actions runs formatting, strict Clippy, and tests on Linux and Windows for branch pushes and pull requests. After checks pass, native Ubuntu 22.04 runners build Linux release archives for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. The ARM64 runner also runs the release-mode tests. Both binaries run offline startup checks. Rust dependencies and build outputs are cached, and dependency resolution uses `--locked`.

Download `mornye-<target>` from the successful **Rust checks** run's artifacts (retained for 14 days). The ARM64 archive is the one for the verified 64-bit B860H. Windows is tested but is not packaged; ARMv7 is not part of this release matrix. Ubuntu 22.04 gives a glibc 2.35 build baseline; these archives do not target older glibc systems or musl/Alpine.

Pushing a `v*` tag runs the same checks and builds, then publishes a GitHub Release only if all jobs pass. The release contains both `.tar.gz` archives and `SHA256SUMS`. No personal access token or application credentials are needed: only the publication job receives the workflow's built-in `contents: write` permission. Actions are pinned to commit SHAs. The workflow does not deploy or restart the bot.

Before tagging, reconcile any device-only source changes with GitHub and choose the exact commit intended for deployment. A workflow cannot recover unpublished code from the running device. No release tag was created while setting up this pipeline. To release an agreed commit later:

```sh
git tag -a v<VERSION> <COMMIT> -m 'Release v<VERSION>'
git push origin v<VERSION>
```

Each archive includes `mornye`, the example config, README, and `BUILD-INFO.txt` identifying the exact source commit, target, ref, and compiler. After downloading both archives and the manifest into one directory, run `sha256sum --check SHA256SUMS` before extracting. Checksums detect corruption; they are not a cryptographic signature of the publisher. Preserve your existing config, credentials, and complete state directory when installing an update. The release's source provenance identifies what was built; it does not establish that the deployed bot already matches it.


Future work: streamed conversational output, interruptible concurrent chat/jobs, native Gemini and ChatGPT subscription adapters, mem0 integration, evaluation-backed skill promotion, richer graph resolution, and specialized deployment verification. ChatGPT subscription access is not implemented by pointing the generic API adapter at a different URL.

The original architecture research is in [docs/research-and-design.md](docs/research-and-design.md).
