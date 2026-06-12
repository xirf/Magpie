# Magpie 🐦‍⬛

Magpie is a modular, performant chatbot that allows you to control your torrent client directly through **Telegram** and/or **Discord**. Optimized specifically for low-resource ARM boards (Set-Top Boxes, Raspberry Pi, Orange Pi, etc.).

Magpie supports both **qBittorrent** (Web API) and **Transmission** (JSON-RPC daemon) out of the box.

---

## Features

* **Multi-Platform Support**: Run the bot on Telegram, Discord, or both simultaneously.
* **Dual Client Engine**: Supports **qBittorrent** and **Transmission**.
* **Torrent Control**:
  * List active, completed, or downloading torrents.
  * Pause, Resume, and Delete torrents (with options to keep or delete downloaded data).
  * Assign labels/categories dynamically.
  * Toggle Alternate Speed Limits (alt-speed).
* **Embedded Download Web Server**:
  * **Direct Download**: Allows local network direct download link generation, avoiding Cloudflare bandwidth round-trips for S3 uploads.
  * **Timer-Based Secure Links**: Links are generated with a secure random token cached in SQLite that automatically expires.
  * **On-the-Fly Zipping**: Allows downloading whole directories compressed as `.zip` on-the-fly.
  * **Resource Protection**: Restricts zipping tasks to a concurrency limit of **1** (using a semaphore) to safeguard low-resource hardware from CPU/IO locks.
  * **Range-Request Support**: Supports standard `Range` headers for seeking and streaming video/audio directly in browser players.
* **Automatic Cloud Upload**: Optionally upload finished torrent files to S3-compatible cloud storage and delete local copies to save disk space.

---

## Configuration

The bot is configured using a `data/config.yml` file at the root. Copy the provided template to get started:

```bash
cp data/config.example.yml data/config.yml
```

### YAML Schema (`data/config.yml`)

```yaml
client:
  type: qbittorrent               # Client type: 'qbittorrent' or 'transmission'
  host: http://localhost:8080/
  user: admin
  password: adminadmin

telegram:
  enabled: true
  bot_token: PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE
  proxy: null                    # Optional SOCKS or HTTP proxy connection string settings

discord:
  enabled: true
  token: PUT_YOUR_DISCORD_BOT_TOKEN_HERE

redis:
  url: null                      # Optional Redis connection URL. Defaults to in-memory emulator.

s3:
  enabled: false                 # Set to true to enable S3 uploads on completion
  endpoint: null                 # e.g., https://s3.us-east-1.amazonaws.com or custom MinIO endpoint
  public_url: null               # Optional public download URL (defaults to endpoint)
  access_key: null
  secret_key: null
  bucket: null
  region: null
  link_expiry: 3600              # Presigned URL expiry time in seconds
  mode: mount                    # 'mount' (presigned links to locally mounted paths) or 'upload' (upload to S3)

local_server:
  enabled: false                 # Set to true to run the embedded download web server
  base_url: http://localhost/    # Public-facing URL pointing to the web server
  bind_addr: 0.0.0.0:3000        # Address and port for the web server to bind to
  link_expiry: 3600              # Expiry time for download links in seconds (default: 1 hour)

seed_after_download: always      # Seeding policy: 'always', 'never', or 'admin_only'

users:
  - user_id: 123456789             # Telegram user ID (optional)
    discord_id: "9876543210123"    # Discord User Snowflake ID (optional)
    role: administrator            # Role: administrator, manager, reader
    notify: true                   # Receive DM notification when torrent finishes downloading
    locale: en                     # User language: en, es, it, pt, ru, uk
    notification_filter: []        # Limit notifications to specific torrent categories/labels
```

---

## Build & Run Instructions

### Prerequisites
* Rust toolchain (Cargo) installed.

### 1. Build the Bot
Build the optimized release binary:
```bash
cargo build --release
```

### 2. Run the Bot
Make sure you configured `data/config.yml`, then execute the binary:
```bash
# On Linux / macOS
./target/release/magpie

# On Windows
.\target\release\magpie.exe
```

### 3. Run Unit Tests
To run the test suites:
```bash
cargo test
```

---

## Cross-Compiling for ARM (Set-Top Box / Raspberry Pi)

To cross-compile from Windows/macOS/Linux to ARM64 (`aarch64-unknown-linux-musl`), you can use `cargo-zigbuild`:

```bash
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

The compiled binary will reside at `target/aarch64-unknown-linux-musl/release/magpie`.

---

## Disclaimer

This software is provided for educational and administrative purposes only. The creators and contributors of Magpie are not responsible for any content downloaded or managed via this bot. Users are solely responsible for ensuring that their use of client engines and this bot complies with all copyright laws, local regulations, and terms of service.

---

## License

This project is licensed under the [MIT License](LICENSE).
