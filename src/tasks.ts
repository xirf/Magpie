import { Bot } from 'grammy';
import { BotContext } from './telegram';
import { RedisWrapper } from './redis_helper';
import { Settings, UserSettings } from './settings';
import { ClientRepo } from './client_manager';
import { escapeMarkdown } from './utils';
import { t } from './i18n';
import { watch } from 'fs';
import { Client as DiscordClient, ActionRowBuilder, ButtonBuilder, ButtonStyle } from 'discord.js';
import { generatePresignedUrl, uploadFolderOrFileToS3 } from './utils/s3';
import { isNotificationSent, markNotificationSent } from './utils/db';

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
      const existsInSqlite = isNotificationSent(torrent.hash);
      if (!exists && !existsInSqlite) {
        let downloadLink = '';

        if (settings.s3.enabled) {
          try {
            console.log(`[S3] Processing completed torrent: ${torrent.name}`);
            const contentPath = torrent.content_path;
            if (!contentPath) {
              throw new Error('Torrent content path is missing.');
            }

            if (settings.s3.mode === 'upload') {
              console.log(`[S3] Uploading ${contentPath} to bucket...`);
              await uploadFolderOrFileToS3(settings, contentPath, '');
              console.log(`[S3] Upload completed successfully for ${torrent.name}`);
            }

            // Find the key to sign
            const { statSync, readdirSync } = await import('fs');
            const { join, basename, relative } = await import('path');

            let s3Key = torrent.name;
            if (contentPath && statSync(contentPath).isDirectory()) {
              const getFiles = (dir: string): string[] => {
                const list = readdirSync(dir);
                let files: string[] = [];
                for (const file of list) {
                  const full = join(dir, file);
                  if (statSync(full).isDirectory()) {
                    files = files.concat(getFiles(full));
                  } else {
                    files.push(full);
                  }
                }
                return files;
              };
              const files = getFiles(contentPath);
              if (files.length > 0) {
                let largestFile = files[0];
                let largestSize = 0;
                for (const file of files) {
                  const size = statSync(file).size;
                  if (size > largestSize) {
                    largestSize = size;
                    largestFile = file;
                  }
                }
                const baseParent = join(contentPath, '..');
                s3Key = relative(baseParent, largestFile).replace(/\\/g, '/');
              }
            } else if (contentPath) {
              s3Key = basename(contentPath);
            }

            downloadLink = await generatePresignedUrl(settings, s3Key);
            console.log(`[S3] Generated presigned download link for ${torrent.name}: ${downloadLink}`);

            if (settings.s3.mode === 'upload') {
              try {
                await manager.delete_one_data(torrent.hash);
                console.log(`[S3] Deleted local torrent and data for ${torrent.name}`);
              } catch (delErr) {
                console.error(`[S3] Failed to delete local torrent data:`, delErr);
              }
            }
          } catch (s3Err) {
            console.error(`[S3] Failed to process S3 upload/link for ${torrent.name}:`, s3Err);
            continue; // Skip notifying/marking done so it can retry later
          }
        }

        const targetUsers = userFilters(settings.users, torrent.category);
        for (const user of targetUsers) {
          if (user.notify) {
            const userLang = user.locale || 'en';
            let message = t(
              "Torrent {name} has finished downloading!",
              userLang,
              { name: escapeMarkdown(torrent.name) }
            );

            if (downloadLink && !user.discord_id) {
              message += `\n\n🔗 **[Download Link](${downloadLink})** *(Expires in 1 hour)*`;
            }

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
                  if (downloadLink) {
                    const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
                      new ButtonBuilder()
                        .setLabel('Download')
                        .setURL(downloadLink)
                        .setStyle(ButtonStyle.Link)
                    );
                    await dcUser.send({ content: message, components: [row] });
                  } else {
                    await dcUser.send(message);
                  }
                }
              } catch (e) {
                console.error(`Failed to notify Discord user ${user.discord_id} of finished torrent:`, e);
              }
            }
          }
        }

        // Apply seeding policy
        let shouldPause = false;
        const seedPolicy = settings.seed_after_download || 'always';
        if (seedPolicy === 'never') {
          shouldPause = true;
        } else if (seedPolicy === 'admin_only') {
          const hasAdmin = targetUsers.some(u => u.role === 'administrator');
          if (!hasAdmin) {
            shouldPause = true;
          }
        }

        const isUploadedAndDeleted = settings.s3.enabled && settings.s3.mode === 'upload';
        if (shouldPause && !isUploadedAndDeleted) {
          try {
            await manager.pause(torrent.hash);
            console.log(`[Tasks] Paused completed torrent "${torrent.name}" to stop seeding per policy: ${seedPolicy}`);
          } catch (pauseErr) {
            console.error(`[Tasks] Failed to pause torrent "${torrent.name}":`, pauseErr);
          }
        }

        // Save to redis for 10 days (10 * 86400 seconds)
        await redis.set(torrent.hash, 'true', 10 * 86400);
        markNotificationSent(torrent.hash);
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
