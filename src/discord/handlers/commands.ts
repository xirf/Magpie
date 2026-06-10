import {
  REST,
  Routes,
  ChatInputCommandInteraction,
  EmbedBuilder,
  Client
} from 'discord.js';
import { UserSettings, Settings } from '../../settings';
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
    {
      name: 'seeding',
      description: 'Set seeding policy after download completes (Admin only)',
      options: [
        {
          name: 'policy',
          description: 'Seeding policy: always, never, or admin_only',
          type: 3, // String type
          required: false,
          choices: [
            { name: 'Always', value: 'always' },
            { name: 'Never', value: 'never' },
            { name: 'Admin Only', value: 'admin_only' }
          ]
        }
      ]
    },
  ];

  try {
    console.log('Registering Discord slash commands...');

    // Clear global commands to avoid duplicates (global + guild = two sets of commands)
    await rest.put(
      Routes.applicationCommands(client.user!.id),
      { body: [] }
    );

    // Only register guild-specific commands for instant updates
    const guilds = await client.guilds.fetch();
    for (const [guildId, guild] of guilds) {
      console.log(`Registering guild commands for: ${guild.name} (${guildId})`);
      await rest.put(
        Routes.applicationGuildCommands(client.user!.id, guildId),
        { body: commandsData }
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
  user: UserSettings,
  settings: Settings
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

    const seedPolicy = settings.seed_after_download || 'always';
    const seedPolicyLabels: Record<string, string> = {
      always: '🌱 Always',
      never: '🚫 Never',
      admin_only: '👑 Admin Only'
    };

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
      .setDescription(statsText)
      .addFields(
        { name: '🌿 Seeding Policy', value: seedPolicyLabels[seedPolicy] ?? seedPolicy, inline: true }
      );

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
  } else if (commandName === 'seeding') {
    if (user.role !== 'administrator') {
      await interaction.reply({ content: translate(user, 'You are not authorized to use this bot'), ephemeral: true });
      return;
    }
    const currentPolicy = settings.seed_after_download || 'always';
    const policyArg = interaction.options.getString('policy');
    if (!policyArg) {
      // No argument: just show current status
      const policyLabels: Record<string, string> = {
        always: '🌱 Always (seed after every download)',
        never: '🚫 Never (stop seeding immediately)',
        admin_only: '👑 Admin Only (seed only when an admin downloads)'
      };
      await interaction.reply({
        content: `**Current seeding policy:** ${policyLabels[currentPolicy] ?? currentPolicy}`,
        ephemeral: true
      });
      return;
    }
    const policy = policyArg as 'always' | 'never' | 'admin_only';
    const prevPolicy = currentPolicy;
    settings.seed_after_download = policy;
    settings.exportSettings();
    const policyLabels: Record<string, string> = {
      always: '🌱 Always',
      never: '🚫 Never',
      admin_only: '👑 Admin Only'
    };
    await interaction.reply({
      content: `✅ Seeding policy updated:\n**${policyLabels[prevPolicy] ?? prevPolicy}** → **${policyLabels[policy] ?? policy}**`,
      ephemeral: true
    });
  }
}
