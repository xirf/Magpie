import { Bot, Context } from 'grammy';
import { RedisWrapper } from '../redis_helper';
import { Settings, UserSettings } from '../settings';
import { t } from '../i18n';
import { commandsComposer } from './handlers/commands';
import { callbackComposer } from './handlers/callbacks';
import { messageComposer } from './handlers/on_message';

export interface BotContext extends Context {
  redis: RedisWrapper;
  settings: Settings;
  user?: UserSettings;
  t: (key: string, vars?: Record<string, any>) => string;
}

export function initBot(settings: Settings, redis: RedisWrapper): Bot<BotContext> {
  // Configure proxy via HTTPS_PROXY env var for native fetch in Bun
  if (settings.telegram.proxy) {
    const proxyStr = Settings.getProxyConnectionString(settings.telegram.proxy);
    process.env.HTTPS_PROXY = proxyStr;
    process.env.HTTP_PROXY = proxyStr;
    console.log(`Configuring bot traffic proxy: ${proxyStr}`);
  }

  const bot = new Bot<BotContext>(settings.telegram.bot_token);

  // Global Context Injection Middleware
  bot.use(async (ctx, next) => {
    ctx.redis = redis;
    ctx.settings = settings;

    const userId = ctx.from?.id;
    if (userId) {
      ctx.user = settings.users.find(u => u.user_id === userId) ||
                 settings.users.find(u => u.user_id === 0 || u.user_id === null || u.user_id === undefined);
    }

    ctx.t = (key: string, vars?: Record<string, any>) => {
      const locale = ctx.user?.locale || ctx.from?.language_code || 'en';
      return t(key, locale, vars);
    };

    // User authorization filter
    if (!ctx.user) {
      // Respond to unauthorized messages
      if (ctx.message || ctx.callbackQuery) {
        const markup = {
          inline_keyboard: [[
            { text: 'Github', url: 'https://github.com/ch3p4ll3/QBittorrentBot/' }
          ]]
        };
        const text = ctx.t("You are not authorized to use this bot");
        if (ctx.message) {
          await ctx.reply(text, { reply_markup: markup });
        } else if (ctx.callbackQuery) {
          await ctx.answerCallbackQuery({ text, show_alert: true });
        }
      }
      return; // Stop update processing
    }

    await next();
  });

  // Mount composers
  bot.use(commandsComposer);
  bot.use(callbackComposer);
  bot.use(messageComposer);

  return bot;
}
