import {
  StringSelectMenuInteraction,
  ButtonInteraction,
  ButtonBuilder,
  ButtonStyle,
  ActionRowBuilder
} from 'discord.js';
import { UserSettings, Settings, UserRole } from '../../settings';
import { QBittorrentManager } from '../../client_manager/qbittorrent';
import { showTorrentDetails, showTorrentList } from './common';
import { join } from 'path';
import { unlinkSync } from 'fs';

export async function handleInteraction(
  interaction: StringSelectMenuInteraction | ButtonInteraction,
  manager: QBittorrentManager,
  settings: Settings
) {
  const user = settings.users.find(u => u.discord_id === interaction.user.id) ||
               settings.users.find(u => u.discord_id === null || u.discord_id === undefined) ||
               { user_id: 0, discord_id: interaction.user.id, role: 'reader', locale: 'en' } as UserSettings;
  if (interaction.isStringSelectMenu()) {
    if (interaction.customId === 'select_torrent') {
      const hash = interaction.values[0];
      await showTorrentDetails(interaction, hash, manager, user);
    }
  } else if (interaction.isButton()) {
    const customId = interaction.customId;

    if (customId === 'dc_back_list') {
      await showTorrentList(interaction, manager, user);
    } else if (customId === 'dc_retry') {
      await handleRetry(interaction, manager, user);
    } else if (customId === 'dc_request_access') {
      await handleRequestAccess(interaction as ButtonInteraction, settings);
    } else if (customId.startsWith('dc_auth:')) {
      await handleAuthInteraction(interaction as ButtonInteraction, settings);
    } else if (customId.startsWith('dc_status:')) {
      const status = customId.split(':')[1];
      await showTorrentList(interaction, manager, user, status);
    } else if (customId.startsWith('dc_pause:')) {
      if (user.role === 'reader') {
        await interaction.reply({ content: 'Unauthorized.', ephemeral: true });
        return;
      }
      const hash = customId.split(':')[1];
      await manager.pause(hash);
      await new Promise(resolve => setTimeout(resolve, 1500));
      await showTorrentDetails(interaction, hash, manager, user);
    } else if (customId.startsWith('dc_resume:')) {
      if (user.role === 'reader') {
        await interaction.reply({ content: 'Unauthorized.', ephemeral: true });
        return;
      }
      const hash = customId.split(':')[1];
      await manager.resume(hash);
      await new Promise(resolve => setTimeout(resolve, 1500));
      await showTorrentDetails(interaction, hash, manager, user);
    } else if (customId.startsWith('dc_delete:')) {
      if (user.role !== 'administrator') {
        await interaction.reply({ content: 'Unauthorized. Only administrators can delete torrents.', ephemeral: true });
        return;
      }
      const hash = customId.split(':')[1];
      const confirmNoDataBtn = new ButtonBuilder()
        .setCustomId(`dc_cldel:${hash}:false`)
        .setLabel('Delete (Keep Files)')
        .setStyle(ButtonStyle.Danger);

      const confirmDataBtn = new ButtonBuilder()
        .setCustomId(`dc_cldel:${hash}:true`)
        .setLabel('Delete EVERYTHING')
        .setStyle(ButtonStyle.Danger);

      const cancelBtn = new ButtonBuilder()
        .setCustomId(`dc_detail:${hash}`)
        .setLabel('Cancel')
        .setStyle(ButtonStyle.Secondary);

      const row = new ActionRowBuilder<ButtonBuilder>().addComponents(confirmNoDataBtn, confirmDataBtn, cancelBtn);
      await interaction.update({
        content: '⚠️ **Are you sure you want to delete this torrent?**',
        embeds: [],
        components: [row]
      });
    } else if (customId.startsWith('dc_cldel:')) {
      if (user.role !== 'administrator') {
        await interaction.reply({ content: 'Unauthorized.', ephemeral: true });
        return;
      }
      const parts = customId.split(':');
      const hash = parts[1];
      const deleteFiles = parts[2] === 'true';

      if (deleteFiles) {
        await manager.delete_one_data(hash);
      } else {
        await manager.delete_one_no_data(hash);
      }

      await interaction.update({
        content: `🗑️ Torrent deleted successfully${deleteFiles ? ' (including files)' : ''}.`,
        components: []
      });
    } else if (customId.startsWith('dc_detail:')) {
      const hash = customId.split(':')[1];
      await showTorrentDetails(interaction, hash, manager, user);
    }
  }
}

async function handleRetry(
  interaction: ButtonInteraction,
  manager: QBittorrentManager,
  user: UserSettings
) {
  if (user.role === 'reader') {
    await interaction.reply({ content: 'Unauthorized.', ephemeral: true });
    return;
  }

  const message = interaction.message;
  if (!message.reference || !message.reference.messageId) {
    await interaction.reply({ content: 'Original message reference not found.', ephemeral: true });
    return;
  }

  const disabledRow = new ActionRowBuilder<ButtonBuilder>().addComponents(
    new ButtonBuilder()
      .setCustomId('dc_retry')
      .setLabel('Retrying...')
      .setStyle(ButtonStyle.Primary)
      .setDisabled(true)
  );

  await interaction.update({
    content: '⏳ Retrying...',
    components: [disabledRow]
  });

  try {
    const channel = interaction.channel || await interaction.client.channels.fetch(interaction.channelId);
    if (!channel || !('messages' in channel)) {
      throw new Error('Could not access channel messages.');
    }
    const originalMessage = await (channel as any).messages.fetch(message.reference.messageId);
    if (!originalMessage) {
      throw new Error('Original message not found.');
    }

    // 1. Check for magnet links in message content
    const magnetRegex = /magnet:\?xt=urn:[a-zA-Z0-9]+:[^\s]+/i;
    const match = originalMessage.content.match(magnetRegex);
    if (match) {
      const magnetLink = match[0];
      const added = await manager.add_magnet(magnetLink);
      if (added) {
        await interaction.editReply({
          content: '✅ Magnet link added successfully!',
          components: []
        });
      } else {
        throw new Error('Failed to add magnet link.');
      }
      return;
    }

    // 2. Check for .torrent file attachments
    if (originalMessage.attachments.size > 0) {
      const torrentAttachment = originalMessage.attachments.find((att: any) => att.name.endsWith('.torrent'));
      if (torrentAttachment) {
        const res = await fetch(torrentAttachment.url);
        const arrayBuffer = await res.arrayBuffer();
        const tempPath = join(import.meta.dir, `../../../data/temp_${Date.now()}_${torrentAttachment.name}`);
        await Bun.write(tempPath, Buffer.from(arrayBuffer));

        try {
          const added = await manager.add_torrent(tempPath);
          if (added) {
            await interaction.editReply({
              content: '✅ Torrent file added successfully!',
              components: []
            });
          } else {
            throw new Error('Failed to add torrent file.');
          }
        } finally {
          try {
            unlinkSync(tempPath);
          } catch {}
        }
        return;
      }
    }

    throw new Error('No valid magnet link or torrent file found in the original message.');
  } catch (err) {
    console.error('Error retrying addition:', err);
    const retryRow = new ActionRowBuilder<ButtonBuilder>().addComponents(
      new ButtonBuilder()
        .setCustomId('dc_retry')
        .setLabel('Retry')
        .setStyle(ButtonStyle.Primary)
    );
    const errMsg = err instanceof Error ? err.message : String(err);
    if (err instanceof Error && err.message.includes('409')) {
      await interaction.editReply({
        content: '⚠️ This torrent/magnet link is already in the download list.',
        components: [retryRow]
      });
    } else {
      await interaction.editReply({
        content: `❌ Error: ${errMsg}`,
        components: [retryRow]
      });
    }
  }
}

async function handleRequestAccess(interaction: ButtonInteraction, settings: Settings) {
  await interaction.update({
    content: '⏳ Access request sent to administrators. Please wait...',
    components: []
  });

  for (const u of settings.users) {
    if (u.role === 'administrator' && u.discord_id && u.discord_id !== '9876543210123') {
      try {
        const adminUser = await interaction.client.users.fetch(u.discord_id);
        if (adminUser) {
          const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
            new ButtonBuilder()
              .setCustomId(`dc_auth:approve:administrator:${interaction.user.id}:${interaction.user.username}`)
              .setLabel('Approve Admin')
              .setStyle(ButtonStyle.Success),
            new ButtonBuilder()
              .setCustomId(`dc_auth:approve:manager:${interaction.user.id}:${interaction.user.username}`)
              .setLabel('Approve Manager')
              .setStyle(ButtonStyle.Primary),
            new ButtonBuilder()
              .setCustomId(`dc_auth:approve:reader:${interaction.user.id}:${interaction.user.username}`)
              .setLabel('Approve Reader')
              .setStyle(ButtonStyle.Secondary),
            new ButtonBuilder()
              .setCustomId(`dc_auth:deny:${interaction.user.id}:${interaction.user.username}`)
              .setLabel('Deny')
              .setStyle(ButtonStyle.Danger)
          );
          await adminUser.send({
            content: `🔔 User **${interaction.user.username}** (ID: \`${interaction.user.id}\`) is requesting access to the bot.`,
            components: [row]
          });
        }
      } catch (err) {
        console.error(`Failed to notify admin ${u.discord_id}:`, err);
      }
    }
  }
}

async function handleAuthInteraction(interaction: ButtonInteraction, settings: Settings) {
  const parts = interaction.customId.split(':');
  const action = parts[1]; // 'approve' or 'deny'

  if (action === 'approve') {
    const role = parts[2] as UserRole;
    const targetUserId = parts[3];
    const username = parts[4] || 'User';

    let user = settings.users.find(u => u.discord_id === targetUserId);
    if (!user) {
      user = {
        user_id: 0,
        discord_id: targetUserId,
        role: role,
        locale: 'en',
        notify: true,
        notification_filter: []
      };
      settings.users.push(user);
    } else {
      user.role = role;
    }
    settings.exportSettings();

    await interaction.update({
      content: `✅ Authorized **${username}** (ID: \`${targetUserId}\`) as **${role}**.`,
      components: []
    });

    try {
      const targetUser = await interaction.client.users.fetch(targetUserId);
      if (targetUser) {
        await targetUser.send(`🎉 Your access request has been approved! You now have **${role}** role.`);
      }
    } catch {}
  } else if (action === 'deny') {
    const targetUserId = parts[2];
    const username = parts[3] || 'User';

    await interaction.update({
      content: `❌ Access request for **${username}** (ID: \`${targetUserId}\`) was denied.`,
      components: []
    });

    try {
      const targetUser = await interaction.client.users.fetch(targetUserId);
      if (targetUser) {
        await targetUser.send(`❌ Your access request was denied by an administrator.`);
      }
    } catch {}
  }
}
