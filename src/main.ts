import { join } from 'path';
import { Settings } from './settings';
import { RedisWrapper } from './redis_helper';
import { initBot } from './telegram';
import { torrentFinished, watchConfig } from './tasks';
import { initDiscordBot } from './discord';
import { ClientRepo } from './client_manager';

async function main() {
  const baseDir = join(import.meta.dir, '..');
  console.log("Starting Magpie (TS/Bun version)...");

  // Load configuration settings
  const settings = Settings.loadSettings();

  // Create and connect to Redis client
  const redis = new RedisWrapper(settings.redis.url);
  await redis.connect();

  // Determine active bot providers
  const isTelegramActive = settings.telegram.enabled && 
    settings.telegram.bot_token && 
    settings.telegram.bot_token !== 'PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE';

  const isDiscordActive = settings.discord.enabled && 
    settings.discord.token && 
    settings.discord.token !== 'PUT_YOUR_DISCORD_BOT_TOKEN_HERE';

  if (!isTelegramActive && !isDiscordActive) {
    console.error("\n❌ ERROR: No active bot providers configured!");
    console.error("Please configure a valid Telegram or Discord bot token in data/config.yml.");
    console.error("Make sure to set enabled: true for the provider you want to use.\n");
    process.exit(1);
  }

  // Initialize Telegram bot instance if enabled
  let telegramBot: any = null;
  if (isTelegramActive) {
    console.log("Starting QBittorrent Telegram Bot...");
    telegramBot = initBot(settings, redis);
  } else {
    console.log("Telegram bot is disabled or not configured.");
  }

  // Initialize Discord bot instance if enabled
  let discordBot: any = null;
  if (isDiscordActive) {
    console.log("Starting QBittorrent Discord Bot...");
    const manager = ClientRepo.getClientManager(settings);
    discordBot = initDiscordBot(settings, manager);
  } else {
    console.log("Discord bot is disabled or not configured.");
  }

  // Shared manager instance reused across all poll cycles (avoids re-login overhead)
  const sharedManager = ClientRepo.getClientManager(settings);

  // Schedule periodic completed torrent checks (every 60 seconds)
  const torrentCheckInterval = setInterval(() => {
    torrentFinished(telegramBot, discordBot, redis, settings, sharedManager).catch(err => {
      console.error("Error running periodic torrentFinished check:", err);
    });
  }, 60000);

  // Start watching config.yml for updates
  const configPath = join(baseDir, 'data/config.yml');
  watchConfig(configPath, settings);

  console.log("Magpie is online and polling for updates...");

  // Setup graceful shutdown handlers
  const shutdown = () => {
    console.log("Stopping bot and cleaning up...");
    clearInterval(torrentCheckInterval);
    if (telegramBot) {
      telegramBot.stop();
    }
    if (discordBot) {
      console.log("Stopping Discord bot...");
      discordBot.destroy();
    }
    process.exit(0);
  };

  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);

  // Start active bots
  const startPromises: Promise<any>[] = [];
  if (telegramBot) {
    console.log("Starting Telegram bot polling...");
    startPromises.push(telegramBot.start());
  }
  if (discordBot && settings.discord.token) {
    console.log("Logging in Discord bot...");
    startPromises.push(discordBot.login(settings.discord.token));
  }
  if (startPromises.length > 0) {
    await Promise.all(startPromises);
  }
}

main().catch(err => {
  console.error("Fatal error during bot initialization:", err);
  process.exit(1);
});
