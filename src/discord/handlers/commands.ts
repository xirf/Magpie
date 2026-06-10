import {
  REST,
  Routes,
  ChatInputCommandInteraction,
  EmbedBuilder,
  Client
} from 'discord.js';
import { UserSettings } from '../../settings';
import { QBittorrentManager } from '../../client_manager/qbittorrent';
import { translate, showTorrentList } from './common';
import { convertSize } from '../../utils';
import * as si from 'systeminformation';

export async function registerCommands(client: Client, token: string) {
  const rest = new REST({ version: '10' }).setToken(token);
  const commandsData = [
    {
      name: 'list',
      description: 'List active torrents and manage them',
    },
    {
      name: 'stats',
      description: 'Get host system statistics',
    },
    {
      name: 'speedlimit',
      description: 'Toggle alternate speed limits mode',
    },
  ];

  try {
    console.log('Registering Discord slash commands...');
    // Register globally (propagation can take up to an hour)
    await rest.put(
      Routes.applicationCommands(client.user!.id),
      { body: commandsData }
    );

    // Clear guild-specific commands to avoid duplicates (since global is active)
    const guilds = await client.guilds.fetch();
    for (const [guildId, guild] of guilds) {
      console.log(`Clearing guild-specific commands for guild: ${guild.name} (${guildId}) to prevent duplicates`);
      await rest.put(
        Routes.applicationGuildCommands(client.user!.id, guildId),
        { body: [] }
      );
    }
    console.log('Discord slash commands registered successfully.');
  } catch (error) {
    console.error('Failed to register Discord slash commands:', error);
  }
}

export async function handleCommand(
  interaction: ChatInputCommandInteraction,
  manager: QBittorrentManager,
  user: UserSettings
) {
  const { commandName } = interaction;

  if (commandName === 'list') {
    await showTorrentList(interaction, manager, user);
  } else if (commandName === 'stats') {
    await interaction.deferReply();
    let cpuTemp = 0;
    try {
      const temp = await si.cpuTemperature();
      cpuTemp = temp.main || 0;
    } catch {}

    let cpuUsage = 0;
    try {
      const load = await si.currentLoad();
      cpuUsage = Math.round(load.currentLoad);
    } catch {}

    let freeMemory = 0;
    let totalMemory = 0;
    let memoryPercent = 0;
    try {
      const mem = await si.mem();
      freeMemory = mem.available;
      totalMemory = mem.total;
      memoryPercent = Math.round(((mem.total - mem.available) / mem.total) * 100);
    } catch {}

    let diskUsed = 0;
    let diskTotal = 0;
    let diskPercent = 0;
    try {
      const disks = await si.fsSize();
      const mntDisk = disks.find(d => d.mount === '/mnt') || disks[0];
      if (mntDisk) {
        diskUsed = mntDisk.used;
        diskTotal = mntDisk.size;
        diskPercent = Math.round(mntDisk.use);
      }
    } catch {}

    const statsText = translate(user,
      "**============SYSTEM============**\n**CPU Usage:** {cpu_usage}%\n" +
      "**CPU Temp:** {cpu_temp}°C\n**Free Memory:** {free_memory} of {total_memory} ({memory_percent}%)\n" +
      "**Disks usage:** {disk_used} of {disk_total} ({disk_percent}%)",
      {
        cpu_usage: cpuUsage,
        cpu_temp: cpuTemp,
        free_memory: convertSize(freeMemory),
        total_memory: convertSize(totalMemory),
        memory_percent: memoryPercent,
        disk_used: convertSize(diskUsed),
        disk_total: convertSize(diskTotal),
        disk_percent: diskPercent,
      }
    );

    const embed = new EmbedBuilder()
      .setTitle(translate(user, 'System Statistics'))
      .setColor(0x00ae86)
      .setDescription(statsText);

    await interaction.editReply({ embeds: [embed] });
  } else if (commandName === 'speedlimit') {
    if (user.role === 'reader') {
      await interaction.reply({ content: translate(user, 'You are not authorized to use this bot'), ephemeral: true });
      return;
    }
    await interaction.deferReply();
    const active = await manager.toggle_speed_limit();
    const modeStr = active ? 'ON' : 'OFF';
    await interaction.editReply({ content: `Alternate speed limits toggled: **${modeStr}**` });
  }
}
