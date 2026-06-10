import { InlineKeyboard } from 'grammy';
import { BotContext } from '../index';
import { ClientRepo } from '../../client_manager';

export async function sendMenu(ctx: BotContext, messageId?: number): Promise<void> {
  const userId = ctx.from?.id;
  if (!userId) return;

  const user = ctx.user!;
  const role = user.role;

  // Build inline keyboard
  const keyboard = new InlineKeyboard();
  keyboard.text(ctx.t("📝 List"), "list:").row();

  if (role === 'manager' || role === 'administrator') {
    keyboard
      .text(ctx.t("➕ Add Magnet"), "category:add_magnet")
      .text(ctx.t("➕ Add Torrent"), "category:add_torrent")
      .row()
      .text(ctx.t("⏯ Pause/Resume"), "menu_pause_resume:")
      .row();
  }

  if (role === 'administrator') {
    keyboard
      .text(ctx.t("🗑 Delete"), "menu_delete:")
      .row()
      .text(ctx.t("📂 Categories"), "menu_categories:")
      .row()
      .text(ctx.t("⚙️ Settings"), "settings:")
      .row();
  }

  await ctx.redis.set(`action:${userId}`, '');

  const text = ctx.t("Welcome to QBittorrent Bot");

  try {
    if (messageId && ctx.chat) {
      await ctx.api.editMessageText(ctx.chat.id, messageId, text, {
        reply_markup: keyboard,
      });
    } else {
      await ctx.reply(text, { reply_markup: keyboard });
    }
  } catch (e) {
    console.warn(`Failed to edit menu message: ${e}. Sending a new one.`);
    await ctx.reply(text, { reply_markup: keyboard });
  }
}

export async function listActiveTorrents(
  ctx: BotContext,
  messageId: number,
  callbackPrefix?: string,
  statusFilter?: string | null
): Promise<void> {
  const settings = ctx.settings;
  const manager = ClientRepo.getClientManager(settings);

  let torrents: any[] = [];
  try {
    torrents = await manager.get_torrents(null, statusFilter);
  } catch (e) {
    console.error("Failed to retrieve torrents list", e);
  }

  const keyboard = new InlineKeyboard();

  // Active status filter buttons at the top
  const activeDl = statusFilter === 'downloading' ? '*' : '';
  const activeComp = statusFilter === 'completed' ? '*' : '';
  const activePause = statusFilter === 'paused' ? '*' : '';

  keyboard
    .text(ctx.t("⏳ {active} Downloading", { active: activeDl }), "by_status_list:downloading")
    .text(ctx.t("✔️ {active} Completed", { active: activeComp }), "by_status_list:completed")
    .text(ctx.t("⏸️ {active} Paused", { active: activePause }), "by_status_list:paused")
    .row();

  const textNoTorrents = ctx.t("There are no torrents");
  const textBackMenu = ctx.t("🔙 Menu");

  if (torrents.length === 0) {
    keyboard.text(textBackMenu, "menu:").row();
    try {
      if (ctx.chat) {
        await ctx.api.editMessageText(ctx.chat.id, messageId, textNoTorrents, {
          reply_markup: keyboard,
        });
      }
    } catch {
      await ctx.reply(textNoTorrents, { reply_markup: keyboard });
    }
    return;
  }

  // List each torrent as a button
  for (const torrent of torrents) {
    const btnText = torrent.name.substring(0, 40);
    const cbData = callbackPrefix
      ? `${callbackPrefix}:${torrent.hash}`
      : `torrentInfo:${torrent.hash}`;
    keyboard.text(btnText, cbData).row();
  }

  keyboard.text(textBackMenu, "menu:").row();

  try {
    if (ctx.chat) {
      await ctx.api.editMessageReplyMarkup(ctx.chat.id, messageId, {
        reply_markup: keyboard,
      });
    }
  } catch {
    await ctx.reply(textBackMenu, { reply_markup: keyboard });
  }
}

export async function listCategories(
  ctx: BotContext,
  messageId: number,
  callbackPrefix: string
): Promise<void> {
  const settings = ctx.settings;
  const manager = ClientRepo.getClientManager(settings);
  const categories = await manager.get_categories();

  const keyboard = new InlineKeyboard();

  if (!categories || categories.length === 0) {
    keyboard.text(ctx.t("🔙 Menu"), "menu:").row();
    const textNoCategories = ctx.t("There are no categories");
    try {
      if (ctx.chat) {
        await ctx.api.editMessageText(ctx.chat.id, messageId, textNoCategories, {
          reply_markup: keyboard,
        });
      }
    } catch {
      await ctx.reply(textNoCategories, { reply_markup: keyboard });
    }
    return;
  }

  for (const cat of categories) {
    keyboard.text(cat, `${callbackPrefix}:${cat}`).row();
  }

  keyboard.text(ctx.t("🔙 Menu"), "menu:").row();

  const textChoose = ctx.t("Choose a category:");
  try {
    if (ctx.chat) {
      await ctx.api.editMessageText(ctx.chat.id, messageId, textChoose, {
        reply_markup: keyboard,
      });
    }
  } catch {
    await ctx.reply(textChoose, { reply_markup: keyboard });
  }
}
