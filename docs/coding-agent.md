# Discord coding tasks

Magpie optionally runs one Zot worker against the DeepSeek API, using the existing
Discord connection. Coding is disabled by default. Existing torrent commands and
configuration files remain supported.

## SD-card deployment

Once CI succeeds, run the installer from a checkout on the SD card:

```bash
bash deploy/install-sdcard.sh /home/rustfs_user/rustfs_data/magpie
```

It requires authenticated `gh`, selects a successful build of the exact feature
branch head, verifies checksums, and downloads Magpie and pinned Zot executables
directly onto the SD card. It refuses root-filesystem destinations and replacement
while either Magpie service is active. It does not compile or start services.
Run it as the intended service user, or explicitly adjust ownership afterward.
After merging the feature branch, select mainline builds with `MAGPIE_REF=master`.

The GitHub Actions workflow builds/tests remotely and uploads ARM64 and AMD64
runtime artifacts. Download `magpie-linux-arm64`, verify its included SHA-256
checksum, and extract into `/home/rustfs_user/rustfs_data/magpie/bin/`.
Builds use Ubuntu 24.04: check glibc compatibility on the device. These binaries
are not claimed to be static; use a compatible build container for older OSes.

Download a pinned Zot Linux ARM64 release from
https://github.com/patriceckhart/zot/releases into the same `bin` directory and
verify its release checksum. Confirm that the release supports `zot rpc` and the
documented flags. No Rust or Go compiler is needed on the device.

Copy the saved Magpie configuration to `data/config.yml` on the SD card. Preserve
the original backup until deployment succeeds. Configure your exact Discord ID
as an `administrator` in the existing `users` list.

Put the DeepSeek key in `data/runtime.env` with mode 0600:

```dotenv
DEEPSEEK_API_KEY=replace-with-your-key
```

The provided `deploy/magpie-sdcard.service` runs as `rustfs_user` with all binaries,
data, worktrees, temporary files, and compiler caches on the SD card. Create its
referenced directories, including `data/tmp`, and ensure they are writable by the
service user. Change the service username/paths if needed. Keep keys/configuration
readable only by the service user. Link the unit from its SD-card location using
`systemctl link`, then enable/start `magpie-sdcard.service`. Leave the old
`magpie.service` disabled so only one service owns the bot identity.

## Configuration

Add this section to `data/config.yml`:

```yaml
coding:
  enabled: true
  zot_binary: /home/rustfs_user/rustfs_data/magpie/bin/zot
  state_dir: /home/rustfs_user/rustfs_data/magpie/data/coding
  api_key_env: DEEPSEEK_API_KEY
  model: deepseek-flash
  base_url: https://api.deepseek.com/v1
  allowed_channels:
    - "1559781266382000210"
  max_steps: 40
  timeout_seconds: 1800
  max_tokens: 200000
  projects:
    my-project:
      path: /home/rustfs_user/rustfs_data/projects/my-project
      check: null
```

Coding requires an explicitly configured administrator and allowed channel; it
does not use automatic first-admin enrollment, fallback Telegram identities, or
reader/manager roles. Enabling coding also disables the torrent bot's automatic
first-admin enrollment; configure an administrator before enabling it.
Use a private allowed channel: lifecycle notifications are
posted there. Restart for coding configuration changes. Authorization is checked
both when accepting a command and before executing queued work.

Projects must be Git repository roots. A task starts from committed `HEAD`, not
uncommitted changes in the original checkout. Discord accepts only configured
project aliases. Set `check` to a verification command such as `python3 -m unittest`.
It runs in the worktree after Zot returns. Offload expensive checks using your own
CI submission/wait command; a remote-build dispatcher is not bundled in this release.

## Commands and results

```
/code action:task project:my-project task:Fix the parser crash on empty input
/code action:status
/code action:status id:12
/code action:cancel id:12
```

The queue permits one active job and at most 100 queued jobs. No model process
runs while idle. Each task gets branch `magpie/code-ID` and worktree
`data/coding/worktrees/ID`, retained for review. No automatic pushes, merges,
deployments, or worktree deletion are implemented. Zot is instructed accordingly,
but shell tools have the service user's permissions: use a dedicated non-root
account with access only to intended projects.

- `completed`: Zot returned and configured checks passed.
- `unverified`: Zot returned without a configured check; its answer alone is not
  treated as verified success.
- `failed`: Git, harness, verification, time, token, or output-limit failure.
- `cancelled`: cancellation requested; Magpie stops the process group.
- `interrupted`: restarted during execution. Inspect retained work before
  submitting another task. No interrupted shell command is automatically replayed.

Queued jobs survive restart. API failures are not retried automatically. Task
worktrees, branches, and logs are retained until reviewed and removed manually
with `git worktree remove` and branch/log cleanup.

The token limit counts reported input/output usage after events and can overshoot
by one response; it is not a hard monetary budget. Provider-side spending limits
are separate. JSON frames are limited to 256 KiB and jobs to 10,000 events. Logs
record lifecycle/tool/token metadata only. Prompts remain in local SQLite, so
protect the data directory. Model text, shell output, and keys are not sent to
Discord or copied into event logs.

CI builds both binaries and tests coding-specific behavior without real Discord
or DeepSeek keys. Live DeepSeek compatibility, Discord command registration,
memory measurements, and deployment still need a device smoke test before
unattended operation.
