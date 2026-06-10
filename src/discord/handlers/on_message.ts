import { Message, ChannelType, Client, ActionRowBuilder, ButtonBuilder, ButtonStyle } from 'discord.js';
import { Settings } from '../../settings';
import { QBittorrentManager } from '../../client_manager/qbittorrent';
import { t } from '../../i18n';
import { translate } from './common';
import { join } from 'path';
import { unlinkSync } from 'fs';

export async function handleMessage(
  message: Message,
  client: Client,
  manager: QBittorrentManager,
  settings: Settings
) {
  // Ignore bot messages to avoid infinite loops
  if (message.author.bot) {
    return;
  }

  // Check if DM or the bot is mentioned
  const isDM = message.channel.type === ChannelType.DM;
  const isMentioned = client.user ? message.mentions.has(client.user) : false;

  if (!isDM && !isMentioned) {
    return;
  }

  console.log(`[Discord] Processing message from ${message.author.tag} (${message.author.id}): "${message.content}"`);

  let user = settings.users.find(u => u.discord_id === message.author.id);
  if (!user) {
    const defaultUser = settings.users.find(u => u.discord_id === null || u.discord_id === undefined);
    if (defaultUser) {
      user = defaultUser;
    } else {
      const hasAdmin = settings.users.some(u => u.role === 'administrator' && u.discord_id && u.discord_id !== '9876543210123');
      if (!hasAdmin) {
        user = {
          user_id: 0,
          discord_id: message.author.id,
          role: 'administrator',
          locale: 'en',
          notify: true,
          notification_filter: []
        };
        settings.users.push(user);
        settings.exportSettings();
        await message.reply('👑 You have been automatically authorized as the first **administrator**!');
      } else {
        console.warn(`[Discord] Message author ${message.author.tag} (${message.author.id}) not found in authorized users list.`);
        const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
          new ButtonBuilder()
            .setCustomId('dc_request_access')
            .setLabel('Request Access')
            .setStyle(ButtonStyle.Primary)
        );
        await message.reply({
          content: '❌ You are not authorized to use this bot.',
          components: [row]
        });
        return;
      }
    }
  }

  if (user.role === 'reader') {
    console.warn(`[Discord] Message author ${message.author.tag} is authorized as reader only. Restricting access.`);
    await message.reply(translate(user, 'You are not authorized to use this bot'));
    return;
  }

  // 1. Check for magnet links in message content
  const magnetRegex = /magnet:\?xt=urn:[a-zA-Z0-9]+:[^\s]+/i;
  const match = message.content.match(magnetRegex);
  if (match) {
    const magnetLink = match[0];
    if (typeof (message.channel as any).sendTyping === 'function') {
      await (message.channel as any).sendTyping();
    }
    const retryRow = new ActionRowBuilder<ButtonBuilder>().addComponents(
      new ButtonBuilder()
        .setCustomId('dc_retry')
        .setLabel('Retry')
        .setStyle(ButtonStyle.Primary)
    );
    try {
      const added = await manager.add_magnet(magnetLink);
      if (added) {
        await message.reply('✅ Magnet link added successfully!');
      } else {
        await message.reply({
          content: '❌ Failed to add magnet link.',
          components: [retryRow]
        });
      }
    } catch (e) {
      console.error('Error adding magnet in Discord message handler:', e);
      if (e instanceof Error && e.message.includes('409')) {
        await message.reply({
          content: '⚠️ This torrent/magnet link is already in the download list.',
          components: [retryRow]
        });
      } else {
        await message.reply({
          content: `❌ Error: ${e instanceof Error ? e.message : e}`,
          components: [retryRow]
        });
      }
    }
    return;
  }

  // 2. Check for .torrent file attachments
  if (message.attachments.size > 0) {
    const torrentAttachment = message.attachments.find(att => att.name.endsWith('.torrent'));
    if (torrentAttachment) {
      if (typeof (message.channel as any).sendTyping === 'function') {
        await (message.channel as any).sendTyping();
      }
      const retryRow = new ActionRowBuilder<ButtonBuilder>().addComponents(
        new ButtonBuilder()
          .setCustomId('dc_retry')
          .setLabel('Retry')
          .setStyle(ButtonStyle.Primary)
      );
      try {
        const res = await fetch(torrentAttachment.url);
        const arrayBuffer = await res.arrayBuffer();
        const tempPath = join(import.meta.dir, `../../../data/temp_${Date.now()}_${torrentAttachment.name}`);
        await Bun.write(tempPath, Buffer.from(arrayBuffer));

        const added = await manager.add_torrent(tempPath);
        try {
          unlinkSync(tempPath);
        } catch {}

        if (added) {
          await message.reply('✅ Torrent file added successfully!');
        } else {
          await message.reply({
            content: '❌ Failed to add torrent file.',
            components: [retryRow]
          });
        }
      } catch (e) {
        console.error('Error adding torrent file in Discord message handler:', e);
        if (e instanceof Error && e.message.includes('409')) {
          await message.reply({
            content: '⚠️ This torrent/magnet link is already in the download list.',
            components: [retryRow]
          });
        } else {
          await message.reply({
            content: `❌ Error: ${e instanceof Error ? e.message : e}`,
            components: [retryRow]
          });
        }
      }
    }
  }
}
