import {
  Client,
  GatewayIntentBits,
  Partials,
  Interaction,
  Message
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
      GatewayIntentBits.GuildMessages,
      GatewayIntentBits.DirectMessages,
      GatewayIntentBits.MessageContent
    ],
    partials: [Partials.Channel]
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
      const user = settings.users.find(u => u.discord_id === interaction.user.id) ||
                   settings.users.find(u => u.discord_id === null || u.discord_id === undefined);
      if (!user) {
        console.warn(`[Discord] Unauthorized interaction attempt by ${interaction.user.tag} (${interaction.user.id})`);
        const content = t("You are not authorized to use this bot", 'en');
        if (interaction.isRepliable()) {
          await interaction.reply({ content, ephemeral: true });
        }
        return;
      }

      if (interaction.isChatInputCommand()) {
        console.log(`[Discord] Executing command: /${interaction.commandName}`);
        await handleCommand(interaction, manager, user);
      } else if (interaction.isStringSelectMenu() || interaction.isButton()) {
        const customId = (interaction as any).customId;
        console.log(`[Discord] Executing component interaction: ${customId}`);
        await handleInteraction(interaction, manager, user);
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
