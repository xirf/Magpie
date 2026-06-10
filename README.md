# Magpie (TypeScript + Bun Edition)

Magpie is a modular chatbot built on the fast **Bun** runtime that allows you to control your **qBittorrent** client directly through **Telegram** and/or **Discord**. With this bot, you can manage your torrent downloads, add magnet links or upload torrent files, monitor statistics, and toggle speed limits—all from within your chat.

---

## Features

* **Multi-Platform Support**: Run the bot on Telegram, Discord, or both simultaneously.
* **QBittorrent Control**:
  * List active, completed, or downloading torrents.
  * Pause (Stop), Resume (Start), and Delete torrents.
  * Dynamically assign categories.
  * Toggle Alternate Speed Limits.

---

## Configuration

The bot is configured using a `data/config.yml` file. Copy the provided template to get started:
```bash
cp data/config.example.yml data/config.yml
```

### YAML Schema (`data/config.yml`)

```yaml
client:
  type: qbittorrent
  host: http://localhost:8080/
  user: admin
  password: adminadmin

telegram:
  enabled: true
  bot_token: PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE
  proxy: null # Optional SOCKS or HTTP proxy connection string settings

discord:
  enabled: true
  token: PUT_YOUR_DISCORD_BOT_TOKEN_HERE

redis:
  url: null # Optional Redis connection URL (e.g. redis://localhost:6379/0). Defaults to in-memory emulator.

users:
  - user_id: 123456789             # Telegram user ID (optional)
    discord_id: "9876543210123"    # Discord User Snowflake ID (optional)
    role: administrator            # Role: administrator, manager, reader
    notify: true                   # Receive DM notification when torrent finishes downloading
    locale: en                     # User language: en, es, it, pt, ru, uk
    notification_filter: []        # Limit notifications to specific qBittorrent categories
```

### Roles and Permissions
* **administrator**: Complete access (viewing, adding, pausing/resuming, deleting torrents, CRUD categories, client settings).
* **manager**: Add torrents/magnets, view statistics, and pause/resume torrents.
* **reader**: Read-only access to download list.

---

## Getting Started

### 1. Install Bun
Ensure you have the [Bun Runtime](https://bun.sh) installed:
```bash
# macOS/Linux
curl -fsSL https://bun.sh/install | bash

# Windows (PowerShell)
powershell -c "irm https://bun.sh/install.ps1 | iex"
```

### 2. Install Dependencies
```bash
bun install
```

### 3. Localization Catalog Compilation
The bot uses PO translations compiled into fast-loading JSON objects. Compile them with:
```bash
bun run convert-locales
```

### 4. Running the Bot
Edit `data/config.yml` to insert your qBittorrent host, credentials, and Telegram/Discord tokens. Then run:
```bash
bun start
```

For development mode (restarts on file changes):
```bash
bun run dev
```

---

## Testing

To run the unit test suite and local integration tests:
```bash
bun test
```

---

## Disclaimer

This software is provided for educational and administrative purposes only. The creators and contributors of Magpie are not responsible for any content downloaded or managed via this bot. Users are solely responsible for ensuring that their use of qBittorrent and this bot complies with all applicable copyright laws, local regulations, and terms of service.

---

## License

This project is licensed under the [MIT License](LICENSE).
