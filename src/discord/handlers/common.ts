import {
  EmbedBuilder,
  ActionRowBuilder,
  ButtonBuilder,
  ButtonStyle,
  StringSelectMenuBuilder
} from 'discord.js';
import { UserSettings } from '../../settings';
import { QBittorrentManager } from '../../client_manager/qbittorrent';
import { t } from '../../i18n';
import { convertSize, convertEta, formatProgress } from '../../utils';

export function translate(user: UserSettings | null, key: string, vars?: Record<string, any>): string {
  const locale = user?.locale || 'en';
  return t(key, locale, vars);
}

export async function showTorrentList(
  interactionOrMessage: any,
  manager: QBittorrentManager,
  user: UserSettings | null,
  statusFilter?: string | null
) {
  try {
    const cleanFilter = !statusFilter || statusFilter === 'all' ? null : statusFilter;
    const torrents = await manager.get_torrents(null, cleanFilter);

    // Build the status filter buttons row
    const dlBtn = new ButtonBuilder()
      .setCustomId('dc_status:downloading')
      .setLabel('⏳ Downloading')
      .setStyle(cleanFilter === 'downloading' ? ButtonStyle.Primary : ButtonStyle.Secondary);

    const compBtn = new ButtonBuilder()
      .setCustomId('dc_status:completed')
      .setLabel('✔️ Completed')
      .setStyle(cleanFilter === 'completed' ? ButtonStyle.Primary : ButtonStyle.Secondary);

    const pauseBtn = new ButtonBuilder()
      .setCustomId('dc_status:paused')
      .setLabel('⏸️ Paused')
      .setStyle(cleanFilter === 'paused' ? ButtonStyle.Primary : ButtonStyle.Secondary);

    const allBtn = new ButtonBuilder()
      .setCustomId('dc_status:all')
      .setLabel('📁 All')
      .setStyle(!cleanFilter ? ButtonStyle.Primary : ButtonStyle.Secondary);

    const filterRow = new ActionRowBuilder<ButtonBuilder>().addComponents(allBtn, dlBtn, compBtn, pauseBtn);

    if (torrents.length === 0) {
      const filterName = cleanFilter ? cleanFilter.charAt(0).toUpperCase() + cleanFilter.slice(1) : 'All';
      const content = cleanFilter 
        ? `No torrents found with status: **${filterName}**` 
        : translate(user, 'There are no torrents');

      const components = cleanFilter ? [filterRow] : [];

      if (interactionOrMessage.isButton() || interactionOrMessage.isStringSelectMenu()) {
        await interactionOrMessage.update({ content, embeds: [], components });
      } else {
        await interactionOrMessage.reply({ content, components });
      }
      return;
    }

    const embed = new EmbedBuilder()
      .setTitle(translate(user, 'Welcome to QBittorrent Bot'))
      .setColor(0x00ae86);

    let desc = '';
    for (const t of torrents) {
      const state = t.state.charAt(0).toUpperCase() + t.state.slice(1);
      const progressPercent = Math.round(t.progress * 100);
      const sizeStr = convertSize(t.size);
      const dlSpeedStr = convertSize(t.dlspeed);

      desc += `**${t.name}**\n`;
      desc += `${formatProgress(t.progress)} ${progressPercent}%\n`;
      desc += `State: \`${state}\` | Size: \`${sizeStr}\` | Speed: \`${dlSpeedStr}/s\`\n`;
      desc += `Hash: \`${t.hash}\`\n\n`;
    }
    embed.setDescription(desc.substring(0, 4096));

    const selectMenu = new StringSelectMenuBuilder()
      .setCustomId('select_torrent')
      .setPlaceholder('Select a torrent to manage...');

    for (const t of torrents.slice(0, 25)) {
      selectMenu.addOptions({
        label: t.name.substring(0, 100) || 'Unnamed torrent',
        description: `Size: ${convertSize(t.size)} | Status: ${t.state}`,
        value: t.hash,
      });
    }

    const selectRow = new ActionRowBuilder<StringSelectMenuBuilder>().addComponents(selectMenu);
    const components = [selectRow, filterRow];

    if (interactionOrMessage.isButton() || interactionOrMessage.isStringSelectMenu()) {
      await interactionOrMessage.update({ embeds: [embed], components, content: '' });
    } else {
      await interactionOrMessage.reply({ embeds: [embed], components });
    }
  } catch (error) {
    console.error('Error showing torrent list in Discord:', error);
    const content = 'Failed to fetch torrent list.';
    if (interactionOrMessage.isButton() || interactionOrMessage.isStringSelectMenu()) {
      await interactionOrMessage.update({ content, embeds: [], components: [] }).catch(() => {});
    } else {
      await interactionOrMessage.reply({ content }).catch(() => {});
    }
  }
}

export async function showTorrentDetails(interactionOrMessage: any, hash: string, manager: QBittorrentManager, user: UserSettings | null) {
  try {
    const torrent = await manager.get_torrent(hash);
    if (!torrent) {
      const content = translate(user, 'Torrent not found');
      if (interactionOrMessage.isButton() || interactionOrMessage.isStringSelectMenu()) {
        await interactionOrMessage.update({ content, embeds: [], components: [] });
      } else {
        await interactionOrMessage.reply({ content });
      }
      return;
    }

    const state = torrent.state.charAt(0).toUpperCase() + torrent.state.slice(1);
    const progressPercent = Math.round(torrent.progress * 100);

    const embed = new EmbedBuilder()
      .setTitle(torrent.name)
      .setColor(0x00ae86)
      .addFields(
        { name: 'Progress', value: `${formatProgress(torrent.progress)} ${progressPercent}%`, inline: false },
        { name: 'State', value: `\`${state}\``, inline: true },
        { name: 'Size', value: `\`${convertSize(torrent.size)}\``, inline: true },
        { name: 'Download Speed', value: `\`${convertSize(torrent.dlspeed)}/s\``, inline: true },
        { name: 'ETA', value: `\`${convertEta(torrent.eta)}\``, inline: true },
        { name: 'Category', value: `\`${torrent.category || 'None'}\``, inline: true },
        { name: 'Hash', value: `\`${torrent.hash}\``, inline: false }
      );

    const pauseBtn = new ButtonBuilder()
      .setCustomId(`dc_pause:${torrent.hash}`)
      .setLabel('Pause')
      .setStyle(ButtonStyle.Secondary);

    const resumeBtn = new ButtonBuilder()
      .setCustomId(`dc_resume:${torrent.hash}`)
      .setLabel('Resume')
      .setStyle(ButtonStyle.Success);

    const deleteBtn = new ButtonBuilder()
      .setCustomId(`dc_delete:${torrent.hash}`)
      .setLabel('Delete')
      .setStyle(ButtonStyle.Danger);

    const backBtn = new ButtonBuilder()
      .setCustomId('dc_back_list')
      .setLabel('Back to List')
      .setStyle(ButtonStyle.Secondary);

    const row = new ActionRowBuilder<ButtonBuilder>().addComponents(pauseBtn, resumeBtn, deleteBtn, backBtn);

    if (interactionOrMessage.isButton() || interactionOrMessage.isStringSelectMenu()) {
      await interactionOrMessage.update({ embeds: [embed], components: [row], content: '' });
    } else {
      await interactionOrMessage.reply({ embeds: [embed], components: [row] });
    }
  } catch (error) {
    console.error('Error showing torrent details in Discord:', error);
    const content = 'Failed to fetch torrent details.';
    if (interactionOrMessage.isButton() || interactionOrMessage.isStringSelectMenu()) {
      await interactionOrMessage.update({ content, embeds: [], components: [] }).catch(() => {});
    } else {
      await interactionOrMessage.reply({ content }).catch(() => {});
    }
  }
}
