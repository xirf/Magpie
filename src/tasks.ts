import { Bot } from 'grammy';
import { BotContext } from './telegram';
import { RedisWrapper } from './redis_helper';
import { Settings, UserSettings } from './settings';
import { ClientRepo } from './client_manager';
import { escapeMarkdown } from './utils';
import { t } from './i18n';
import { watch } from 'fs';
import { Client as DiscordClient } from 'discord.js';

function userFilters(users: UserSettings[], category: string | null): UserSettings[] {
  return users.filter(user => {
    if (!user.notification_filter || user.notification_filter.length === 0) {
      return true;
    }
    return category !== null && user.notification_filter.includes(category);
  });
}

export async function torrentFinished(
  bot: Bot<BotContext> | null,
  discordClient: DiscordClient | null,
  redis: RedisWrapper,
  settings: Settings
): Promise<void> {
  try {
    const manager = ClientRepo.getClientManager(settings);
    const completedTorrents = await manager.get_torrents(null, 'completed');

    for (const torrent of completedTorrents) {
      const exists = await redis.exists(torrent.hash);
      if (!exists) {
        const targetUsers = userFilters(settings.users, torrent.category);
        for (const user of targetUsers) {
          if (user.notify) {
            const userLang = user.locale || 'en';
            const message = t(
              "Torrent {name} has finished downloading!",
              userLang,
              { name: escapeMarkdown(torrent.name) }
            );

            // Notify Telegram
            if (user.user_id && bot) {
              try {
                await bot.api.sendMessage(user.user_id, message);
              } catch (e) {
                console.error(`Failed to notify Telegram user ${user.user_id} of finished torrent:`, e);
              }
            }

            // Notify Discord
            if (user.discord_id && discordClient) {
              try {
                const dcUser = await discordClient.users.fetch(user.discord_id);
                if (dcUser) {
                  await dcUser.send(message);
                }
              } catch (e) {
                console.error(`Failed to notify Discord user ${user.discord_id} of finished torrent:`, e);
              }
            }
          }
        }
        // Save to redis for 10 days (10 * 86400 seconds)
        await redis.set(torrent.hash, 'true', 10 * 86400);
      }
    }
  } catch (e) {
    console.error("Error running torrentFinished task:", e);
  }
}

let reloadTimeout: Timer | null = null;

export function watchConfig(path: string, settings: Settings): void {
  try {
    watch(path, (event) => {
      if (event === 'change') {
        if (reloadTimeout) clearTimeout(reloadTimeout);
        reloadTimeout = setTimeout(() => {
          try {
            const newSettings = Settings.loadSettings();
            settings.updateFrom(newSettings);
            console.log("Settings reloaded successfully due to config file change");
          } catch (e) {
            console.error("Failed to reload settings", e);
          }
        }, 100);
      }
    });
  } catch (e) {
    console.error(`Failed to watch config file at ${path}:`, e);
  }
}
