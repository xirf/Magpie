import {
  Client,
  GatewayIntentBits,
  Options,
  Partials,
  Interaction,
  Message,
  ActionRowBuilder,
  ButtonBuilder,
  ButtonStyle
} from 'discord.js';
import { Settings } from '../settings';
import { QBittorrentManager } from '../client_manager/qbittorrent';
import { registerCommands, handleCommand } from './handlers/commands';
import { handleInteraction } from './handlers/interactions';
import { handleMessage } from './handlers/on_message';
import { t } from '../i18n';

export function initDiscordBot(settings: Settings, manager: QBittorrentManager): Client {
  const client = new Client({
    intents: [
      GatewayIntentBits.Guilds,
      GatewayIntentBits.DirectMessages,
      GatewayIntentBits.MessageContent,
    ],
    partials: [Partials.Channel],
    makeCache: Options.cacheWithLimits({
      ...Options.DefaultMakeCacheSettings,
      MessageManager: 0,       // don't cache messages in RAM
      GuildMemberManager: 200, // keep a small member cache
      ReactionManager: 0,
      GuildEmojiManager: 0,
      GuildStickerManager: 0,
      GuildInviteManager: 0,
    }),
    sweepers: {
      ...Options.DefaultSweeperSettings,
    },
  });

  client.once('ready', async () => {
    console.log(`Discord bot is online! Logged in as ${client.user?.tag}`);
    if (settings.discord.token) {
      await registerCommands(client, settings.discord.token);
    }
  });

  client.on('interactionCreate', async (interaction: Interaction) => {
    console.log(`[Discord] Interaction received from ${interaction.user.tag} (${interaction.user.id}): type=${interaction.type}`);
    try {
      let user = settings.users.find(u => u.discord_id === interaction.user.id);

      const isRequestAccess = interaction.isButton() && interaction.customId === 'dc_request_access';
      const isAuthInteraction = interaction.isButton() && interaction.customId.startsWith('dc_auth:');

      if (!user && !isRequestAccess && !isAuthInteraction) {
        // Check if there is any administrator
        const hasAdmin = settings.users.some(u => u.role === 'administrator' && u.discord_id && u.discord_id !== '9876543210123');
        if (!hasAdmin) {
          user = {
            user_id: 0,
            discord_id: interaction.user.id,
            role: 'administrator',
            locale: 'en',
            notify: true,
            notification_filter: []
          };
          settings.users.push(user);
          settings.exportSettings();
          if (interaction.isRepliable()) {
            await interaction.reply({ content: '👑 You have been automatically authorized as the first **administrator**!', ephemeral: true });
          }
        } else {
          console.warn(`[Discord] Unauthorized interaction attempt by ${interaction.user.tag} (${interaction.user.id})`);
          if (interaction.isRepliable()) {
            const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
              new ButtonBuilder()
                .setCustomId('dc_request_access')
                .setLabel('Request Access')
                .setStyle(ButtonStyle.Primary)
            );
            await interaction.reply({
              content: '❌ You are not authorized to use this bot.',
              components: [row],
              ephemeral: true
            });
          }
          return;
        }
      }

      const resolvedUser = user || settings.users.find(u => u.discord_id === null || u.discord_id === undefined) || {
        user_id: 0,
        discord_id: interaction.user.id,
        role: 'reader',
        locale: 'en'
      };

      if (interaction.isChatInputCommand()) {
        console.log(`[Discord] Executing command: /${interaction.commandName}`);
        await handleCommand(interaction, manager, resolvedUser, settings);
      } else if (interaction.isStringSelectMenu() || interaction.isButton()) {
        const customId = (interaction as any).customId;
        console.log(`[Discord] Executing component interaction: ${customId}`);
        await handleInteraction(interaction, manager, settings as any);
      }
    } catch (err) {
      console.error('[Discord] Error handling interaction:', err);
    }
  });

  client.on('messageCreate', async (message: Message) => {
    try {
      await handleMessage(message, client, manager, settings);
    } catch (err) {
      console.error('[Discord] Error handling message:', err);
    }
  });

  return client;
}
