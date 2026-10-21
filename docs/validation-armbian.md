# B860H validation â€” 2026-10-17

Tested on the user's device over SSH with explicit authorization. No model API requests, production deployment, or boot-time service activation were performed.

## Device

- `uname -m`: `aarch64`.
- `getconf LONG_BIT`: `64`.
- OS: Armbian `26.8.0-trunk.105 noble`, Ubuntu 24.04 base.
- Reported total RAM: 1,812 MiB; available at initial inspection: 1,422 MiB.
- Storage: `/dev/mmcblk1p1`, ext4, mounted at `/home/rustfs_user/rustfs_data`.
- Filesystem capacity: 114 GiB, approximately 107 GiB available at inspection.

## Build and execution

Cross-compiled on the development machine with the installed Zig toolchain:

```text
cargo zigbuild --release --locked --target aarch64-unknown-linux-gnu.2.31
cargo zigbuild --release --locked --tests --target aarch64-unknown-linux-gnu.2.31
```

The ARM64 application binary is 9,352,240 bytes (8.92 MiB).

SHA-256: `2e92f0252ccb40f6be7d2e09bbcfa34fc306e6cdc9780d55622a0ce1285b1d3f`

`doctor`, the interactive CLI startup/exit smoke test, and all 13 Rust tests ran successfully on the device. Tests used temporary directories on the microSD and a loopback mock model, with no paid inference. ARM test-suite time: 3.35 seconds, one test thread.

Covered behaviors: exact single-use approval, persistence across restart, wrong-owner rejection, expired approval, denial, changed-file deletion rejection, crash uncertainty without replay, scoped memory and forgetting, inactive skill promotion, Telegram authorization, tool/schema validation, context boundaries, bounded output, nonzero command exit, shell timeout, and stale graph detection.

## Memory observation

Measured from `/proc/<pid>/status` after starting release-mode CLI chat and waiting one second without sending a model request:

| Metric | Value |
| --- | --- |
| VmRSS | 4,052 KiB (~3.96 MiB) |
| VmHWM | 4,052 KiB |
| VmSize | 12,488 KiB |

This is a single idle-process observation. It excludes child tools, builds, a real model conversation, sustained indexing, Telegram activity, and the rest of the OS. It does not establish peak memory, long-term stability, or a seven-day soak result.

## Files on the device

All application and test data are beneath `/home/rustfs_user/rustfs_data/mornye`:

```text
bin/mornye                    ARM64 release application
workspace/                    Rust source, lockfile, README, design docs
state/                        SQLite database, lock, personality
mornye.toml                   absolute workspace/state paths; no secrets
validation/mornye-tests       ARM64 test executable
validation/validate.py        device validation and idle measurement script
validation/results.json       captured raw validation results
tmp/                          validation temporary directory
```

The directory was created with mode 0700 and is owned by root, matching the supplied SSH account. No existing user data was removed. The SSH password is not stored in repository files or deployed configuration.

## Remaining activation steps

The measurements above describe the earlier Telegram/CLI baseline. For the current Discord build, configure `DEEPSEEK_API_KEY`, `DISCORD_BOT_TOKEN` and `discord_user_id`. Keep credentials in a protected environment file, outside the source tree. The staged `deploy/mornye-armbian.service` references `mornye/secrets.env` and keeps working files, temporary files, and application state on the microSD.

The service is staged for review, not installed or enabled. It runs as root with direct host access, matching this test deployment. Review that choice and its exact paths before activation. It requires the microSD mount and imposes a 512 MiB service-group memory ceiling; this is resource control, not sandboxing.


## Discord revision validation, 2026-10-17

After the upstream source audit, the ARM64 release test binary was rebuilt with Serenity 0.12.5 and transferred to `/home/rustfs_user/rustfs_data/mornye/validation/discord-tests-20261017`. All **15 tests passed in 3.52 seconds** on the B860H. Temporary test files used the microSD `mornye/tmp` directory. Added checks cover Discord owner/channel/bot/webhook authorization, UTF-16 chunk limits, durable queue deduplication and interrupted approval recovery. Existing memory/skill tests now also check real message provenance, exact fact deduplication and the separation of model evidence claims from runtime records.

Windows `cargo test --locked`, `cargo fmt --check`, and `cargo clippy --locked --all-targets -- -D warnings` passed. No Discord token or hosted model key was available, so gateway authentication, reconnect behavior against Discord, live chat, and active Discord memory consumption remain unverified. The earlier 4 MiB CLI measurement must not be reused as a Discord service measurement. No service was enabled.


## Memory correction revision validation, 2026-10-17

The memory revision adds explicit `/rate ID useful|wrong` feedback, removes disputed facts from retrieval, and adds `/correct ID replacement` with a superseding lineage event. The replacement records that it came from an explicit user command; it does not fabricate a transcript message ID. A migration adds status and feedback tables to older state and rebuilds the external FTS index. Local coverage checks preserved legacy rows, correction provenance, wrong-fact exclusion, and owner scope.

Windows test result: **17 passed**. `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --check` passed. The ARM64 release build and all 17 tests passed on the B860H (4.61 seconds). The executable passed `doctor` with an isolated validation state directory on the microSD; the existing assistant state was not opened or migrated. Local release SHA-256: `ca51d488aa63f3a0928263b26d7b83306dbe653a54880e4462af3b0b6a9dcb91`. Test-runner SHA-256: `763b6748402492430f48705f6a00610322d9b690c0e45a72fdf9cbac23db4a73`.


## Mornye character profile revision, 2026-10-17

The assistant now installs a Wuthering Waves inspired profile as `MORNYE_PROFILE.md` only when the file is absent. It pairs the profile with the existing user-editable `SOUL.md`, labels game lore as fictional, and preserves user edits across restarts. The profile is a short, original synthesis; it does not reproduce the dialogue transcripts.

The Fandom pages blocked direct retrieval (robots policy / HTTP 402). The profile uses the indexed excerpts and quest summaries available for the two supplied pages, plus the indexed backstory page. The summarized points are Mornye's engineering/professor role, analytical warmth and dry humor, accessible prostheses, ambitions for exploration, her relationships with students, and the Cassette/Void Storm details from the quest. Source links are in `docs/mornye-character-profile.md`.

Windows: **18 tests passed**, Clippy passed with warnings denied, and formatting check passed. On the B860H, all **18 ARM64 tests passed in 5.75 seconds**. The new executable passed `doctor` in a dedicated microSD validation state. Its generated `MORNYE_PROFILE.md` byte-matches the staged profile. Release binary SHA-256: `ee024f3a5adbffc11d1a2a9432809070b1f29971d0972c2158a9158c550c8c76d`. Discord credentials remain unset; this was not a live gateway test and the service remains stopped.
## Discord deployment, 2026-10-18Before deployment, `magpie-sdcard.service` was stopped and disabled. `magpie.service` was already inactive and disabled. Mornye now runs as the enabled systemd unit `mornye-discord-persona.service`. Its Discord bot token was copied from Magpie's microSD config, and its existing DeepSeek key was copied from Magpie's runtime environment into `/home/rustfs_user/rustfs_data/mornye/secrets.env` (`root:root`, mode `0600`). No secret values are recorded here. Mornye now has no runtime dependency on Magpie's files.The deployed config uses the single administrator Discord ID already configured for Magpie as Mornye's owner. No channel ID is configured, so Mornye listens for owner direct messages. Discord's authenticated `GET /users/@me` succeeded for the reused bot token, and the process remained active with the gateway startup logged after restart. No Discord message or hosted model completion was sent during validation.Before opening the existing state, its database was backed up to `/home/rustfs_user/rustfs_data/mornye/backups/mornye.db.pre-discord-deploy-20261018-013936.sqlite3`. The backup and live database both pass SQLite integrity checks. The pre-deployment smoke check ran `doctor` against a clone of the database. On first live start, Mornye added the character profile while preserving `SOUL.md`; the generated profile hash matches the staged source. The unit has a 512 MiB memory limit. `systemd-analyze verify` passed for the Mornye unit; it reported a separate pre-existing warning in `switchit.service`.