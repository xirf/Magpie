import { Composer, InlineKeyboard, InputFile } from 'grammy';
import { BotContext } from '../index';
import { sendMenu, listActiveTorrents, listCategories } from './common';
import { ClientRepo, QBittorrentManager } from '../../client_manager';
import { convertSize, convertEta, formatProgress, escapeMarkdown } from '../../utils';

export const callbackComposer = new Composer<BotContext>();

// Authorization helper functions
function checkAdmin(ctx: BotContext): boolean {
  return ctx.user?.role === 'administrator';
}

function checkManagerOrAdmin(ctx: BotContext): boolean {
  const role = ctx.user?.role;
  return role === 'manager' || role === 'administrator';
}

type CallbackHandler = (
  ctx: BotContext,
  messageId: number,
  arg1: string,
  arg2: string,
  manager: QBittorrentManager
) => Promise<void>;

export type CallbackPrefix =
  | 'menu'
  | 'list'
  | 'by_status_list'
  | 'category'
  | 'add_magnet'
  | 'add_torrent'
  | 'menu_pause_resume'
  | 'pause_all'
  | 'resume_all'
  | 'pause'
  | 'resume'
  | 'menu_delete'
  | 'delete_one'
  | 'delete_one_no_data'
  | 'delete_one_data'
  | 'delete_all'
  | 'delete_all_no_data'
  | 'delete_all_data'
  | 'menu_categories'
  | 'add_category'
  | 'select_category'
  | 'remove_category'
  | 'modify_category'
  | 'settings'
  | 'edit_client'
  | 'toggle_speed_limit'
  | 'check_connection'
  | 'reload_settings'
  | 'torrentInfo'
  | 'export'
  | 'edit_torrent_cat'
  | 'torrent_cat';

const callbackHandlers: Record<CallbackPrefix, CallbackHandler> = {
  menu: async (ctx, messageId) => {
    await sendMenu(ctx, messageId);
  },

  list: async (ctx, messageId) => {
    await listActiveTorrents(ctx, messageId);
  },

  by_status_list: async (ctx, messageId, arg1) => {
    const status = arg1 || null;
    await listActiveTorrents(ctx, messageId, undefined, status);
  },

  category: async (ctx, messageId, arg1, _, manager) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const action = arg1;
    const categories = await manager.get_categories();
    const keyboard = new InlineKeyboard();

    const cbPrefix = action === 'add_magnet' ? 'add_magnet' : 'add_torrent';

    if (categories) {
      for (const cat of categories) {
        keyboard.text(cat, `${cbPrefix}:${cat}`).row();
      }
    }
    keyboard.text("None", `${cbPrefix}:None`).row();
    keyboard.text(ctx.t("🔙 Menu"), "menu:").row();

    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("Choose a category:"), {
      reply_markup: keyboard,
    });
  },

  add_magnet: async (ctx, messageId, arg1) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const category = arg1;
    await ctx.redis.set(`action:${ctx.from!.id}`, `magnet#${category}`);
    await ctx.answerCallbackQuery({ text: ctx.t("Send a magnet link") });

    const keyboard = new InlineKeyboard().text(ctx.t("🔙 Menu"), "menu:").row();
    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("Send a magnet link"), {
      reply_markup: keyboard,
    });
  },

  add_torrent: async (ctx, messageId, arg1) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const category = arg1;
    await ctx.redis.set(`action:${ctx.from!.id}`, `torrent#${category}`);
    await ctx.answerCallbackQuery({ text: ctx.t("Send a torrent file") });

    const keyboard = new InlineKeyboard().text(ctx.t("🔙 Menu"), "menu:").row();
    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("Send a torrent file"), {
      reply_markup: keyboard,
    });
  },

  menu_pause_resume: async (ctx, messageId) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const keyboard = new InlineKeyboard()
      .text(ctx.t("⏸ Pause"), "pause:")
      .text(ctx.t("▶️ Resume"), "resume:")
      .row()
      .text(ctx.t("⏸ Pause All"), "pause_all:")
      .text(ctx.t("▶️ Resume All"), "resume_all:")
      .row()
      .text(ctx.t("🔙 Menu"), "menu:").row();

    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("Pause/Resume a torrent"), {
      reply_markup: keyboard,
    });
  },

  pause_all: async (ctx, _, _1, _2, manager) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    await manager.pause_all();
    await ctx.answerCallbackQuery({ text: ctx.t("Paused all torrents") });
  },

  resume_all: async (ctx, _, _1, _2, manager) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    await manager.resume_all();
    await ctx.answerCallbackQuery({ text: ctx.t("Resumed all torrents") });
  },

  pause: async (ctx, messageId, arg1, _, manager) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const torrentHash = arg1;
    if (!torrentHash) {
      await listActiveTorrents(ctx, messageId, "pause");
    } else {
      await manager.pause(torrentHash);
      await ctx.answerCallbackQuery({ text: ctx.t("Torrent Paused") });
      const torrent = await manager.get_torrent(torrentHash);
      if (torrent) await renderTorrentDetails(ctx, messageId, torrent);
    }
  },

  resume: async (ctx, messageId, arg1, _, manager) => {
    if (!checkManagerOrAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const torrentHash = arg1;
    if (!torrentHash) {
      await listActiveTorrents(ctx, messageId, "resume");
    } else {
      await manager.resume(torrentHash);
      await ctx.answerCallbackQuery({ text: ctx.t("Torrent Resumed") });
      const torrent = await manager.get_torrent(torrentHash);
      if (torrent) await renderTorrentDetails(ctx, messageId, torrent);
    }
  },

  menu_delete: async (ctx, messageId) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const keyboard = new InlineKeyboard()
      .text(ctx.t("🗑 Delete"), "delete_one:")
      .row()
      .text(ctx.t("🗑 Delete All"), "delete_all:")
      .row()
      .text(ctx.t("🔙 Menu"), "menu:").row();

    await ctx.api.editMessageText(ctx.chat!.id, messageId, "Delete a torrent", {
      reply_markup: keyboard,
    });
  },

  delete_one: async (ctx, messageId, arg1) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const torrentHash = arg1;
    if (!torrentHash) {
      await listActiveTorrents(ctx, messageId, "delete_one");
    } else {
      const keyboard = new InlineKeyboard()
        .text(ctx.t("🗑 Delete torrent"), `delete_one_no_data:${torrentHash}`)
        .row()
        .text(ctx.t("🗑 Delete torrent and data"), `delete_one_data:${torrentHash}`)
        .row()
        .text(ctx.t("🔙 Menu"), "menu:").row();

      await ctx.api.editMessageReplyMarkup(ctx.chat!.id, messageId, {
        reply_markup: keyboard,
      });
    }
  },

  delete_one_no_data: async (ctx, messageId, arg1, _, manager) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const torrentHash = arg1;
    await manager.delete_one_no_data(torrentHash);
    await ctx.answerCallbackQuery({ text: ctx.t("Torrent deleted") });
    await sendMenu(ctx, messageId);
  },

  delete_one_data: async (ctx, messageId, arg1, _, manager) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const torrentHash = arg1;
    await manager.delete_one_data(torrentHash);
    await ctx.answerCallbackQuery({ text: ctx.t("Torrent and data deleted") });
    await sendMenu(ctx, messageId);
  },

  delete_all: async (ctx, messageId) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const keyboard = new InlineKeyboard()
      .text(ctx.t("🗑 Delete all torrents"), "delete_all_no_data:")
      .row()
      .text(ctx.t("🗑 Delete all torrents and data"), "delete_all_data:")
      .row()
      .text(ctx.t("🔙 Menu"), "menu:").row();

    await ctx.api.editMessageReplyMarkup(ctx.chat!.id, messageId, {
      reply_markup: keyboard,
    });
  },

  delete_all_no_data: async (ctx, messageId, _, _1, manager) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    await manager.delete_all_no_data();
    await ctx.answerCallbackQuery({ text: ctx.t("Deleted all torrents") });
    await sendMenu(ctx, messageId);
  },

  delete_all_data: async (ctx, messageId, _, _1, manager) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    await manager.delete_all_data();
    await ctx.answerCallbackQuery({ text: ctx.t("Deleted all torrents and data") });
    await sendMenu(ctx, messageId);
  },

  menu_categories: async (ctx, messageId) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const keyboard = new InlineKeyboard()
      .text(ctx.t("➕ Add Category"), "add_category:")
      .row()
      .text(ctx.t("🗑 Remove Category"), "select_category:remove_category")
      .row()
      .text(ctx.t("📝 Modify Category"), "select_category:modify_category")
      .row()
      .text(ctx.t("🔙 Menu"), "menu:").row();

    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("Pause/Resume a download"), {
      reply_markup: keyboard,
    });
  },

  add_category: async (ctx, messageId) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    await ctx.redis.set(`action:${ctx.from!.id}`, 'category_name');
    const keyboard = new InlineKeyboard().text(ctx.t("🔙 Menu"), "menu:").row();

    try {
      await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("Send the category name"), {
        reply_markup: keyboard,
      });
    } catch {
      await ctx.reply(ctx.t("Send the category name"), { reply_markup: keyboard });
    }
  },

  select_category: async (ctx, messageId, arg1) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const action = arg1;
    await listCategories(ctx, messageId, action);
  },

  remove_category: async (ctx, messageId, arg1, _, manager) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const cat = arg1;
    await manager.remove_category(cat);

    const keyboard = new InlineKeyboard().text(ctx.t("🔙 Menu"), "menu:").row();
    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("The category {category_name} has been removed", { category_name: cat }), {
      reply_markup: keyboard,
    });
  },

  modify_category: async (ctx, messageId, arg1) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const cat = arg1;
    await ctx.redis.set(`action:${ctx.from!.id}`, `category_dir_modify#${cat}`);

    const keyboard = new InlineKeyboard().text(ctx.t("🔙 Menu"), "menu:").row();
    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("Send new path for category {category_name}", { category_name: cat }), {
      reply_markup: keyboard,
    });
  },

  settings: async (ctx, messageId) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    const keyboard = new InlineKeyboard()
      .text(ctx.t("📥 Client Settings"), "edit_client:")
      .row()
      .text(ctx.t("🔄 Reload Settings"), "reload_settings:")
      .row()
      .text(ctx.t("🔙 Menu"), "menu:").row();

    await ctx.api.editMessageText(ctx.chat!.id, messageId, ctx.t("QBittorrentBot Settings"), {
      reply_markup: keyboard,
    });
  },

  edit_client: async (ctx, messageId) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    await renderClientSettings(ctx, messageId);
  },

  toggle_speed_limit: async (ctx, messageId, _1, _2, manager) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    await manager.toggle_speed_limit();
    await renderClientSettings(ctx, messageId);
  },

  check_connection: async (ctx, _, _1, _2, manager) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    try {
      const version = await manager.check_connection();
      await ctx.answerCallbackQuery({
        text: ctx.t("✅ The connection works. QBittorrent version: {version}", { version }),
        show_alert: true,
      });
    } catch {
      await ctx.answerCallbackQuery({
        text: ctx.t("❌ Unable to establish connection with QBittorrent"),
        show_alert: true,
      });
    }
  },

  reload_settings: async (ctx) => {
    if (!checkAdmin(ctx)) {
      await ctx.answerCallbackQuery({ text: ctx.t("You are not authorized to use this bot"), show_alert: true });
      return;
    }
    try {
      const { Settings } = require('../../settings');
      const reloaded = Settings.loadSettings();
      ctx.settings.updateFrom(reloaded);
      await ctx.answerCallbackQuery({ text: ctx.t("✅ Settings Reloaded"), show_alert: true });
    } catch (e) {
      console.error("Failed to reload settings", e);
      await ctx.answerCallbackQuery({ text: "Error reloading settings", show_alert: true });
    }
  },

  torrentInfo: async (ctx, messageId, arg1, _, manager) => {
    const torrentHash = arg1;
    const torrent = await manager.get_torrent(torrentHash);
    if (torrent) {
      await renderTorrentDetails(ctx, messageId, torrent);
    } else {
      await ctx.answerCallbackQuery({ text: "Torrent not found", show_alert: true });
    }
  },

  export: async (ctx, _, arg1, _2, manager) => {
    const torrentHash = arg1;
    try {
      const exportData = await manager.export_torrent(torrentHash);
      await ctx.replyWithDocument(new InputFile(exportData.buffer, exportData.name));
      await ctx.answerCallbackQuery();
    } catch (e) {
      console.error("Failed to export torrent file", e);
      await ctx.answerCallbackQuery({ text: "Failed to export torrent", show_alert: true });
    }
  },

  edit_torrent_cat: async (ctx, messageId, arg1) => {
    const torrentHash = arg1;
    await listCategories(ctx, messageId, `torrent_cat:${torrentHash}`);
  },

  torrent_cat: async (ctx, messageId, arg1, arg2, manager) => {
    const torrentHash = arg1;
    const category = arg2;
    await manager.set_torrents_category(category, torrentHash);
    await ctx.answerCallbackQuery({
      text: ctx.t("Torrent category changed to {category}", { category }),
      show_alert: true,
    });
    await sendMenu(ctx, messageId);
  }
};

callbackComposer.on('callback_query:data', async (ctx) => {
  const data = ctx.callbackQuery.data;
  const userId = ctx.from.id;
  const messageId = ctx.callbackQuery.message?.message_id;

  if (!messageId) return;

  const parts = data.split(':');
  const prefix = parts[0];
  const arg1 = parts[1] || '';
  const arg2 = parts[2] || '';

  const manager = ClientRepo.getClientManager(ctx.settings);

  try {
    const handler = callbackHandlers[prefix as CallbackPrefix];
    if (handler) {
      await handler(ctx, messageId, arg1, arg2, manager);
    } else {
      console.warn(`Unknown callback prefix: ${prefix}`);
      await ctx.answerCallbackQuery();
    }
  } catch (e) {
    console.error(`Error in callback execution for prefix ${prefix}`, e);
    await ctx.answerCallbackQuery({ text: "An error occurred", show_alert: true });
  }
});

async function renderClientSettings(ctx: BotContext, messageId: number) {
  const manager = ClientRepo.getClientManager(ctx.settings);
  const speedLimit = await manager.get_speed_limit_mode();
  const speedLimitStatus = speedLimit ? ctx.t("✅ Enabled") : ctx.t("❌ Disabled");

  const confs = ctx.t("**Speed Limit**: {speed_limit_status}", { speed_limit_status: speedLimitStatus });
  const clientType = ctx.settings.client.type.charAt(0).toUpperCase() + ctx.settings.client.type.slice(1);
  const text = ctx.t("Edit {client_type} client settings \n\n{configs}", {
    client_type: clientType,
    configs: confs,
  });

  const keyboard = new InlineKeyboard()
    .text(ctx.t("🐢 Toggle Speed Limit"), "toggle_speed_limit:")
    .row()
    .text(ctx.t("✅ Check Client connection"), "check_connection:")
    .row()
    .text(ctx.t("🔙 Settings"), "settings:").row();

  await ctx.api.editMessageText(ctx.chat!.id, messageId, text, {
    reply_markup: keyboard,
    parse_mode: 'Markdown',
  });
}

async function renderTorrentDetails(ctx: BotContext, messageId: number, torrent: any) {
  let textToSend = `${escapeMarkdown(torrent.name)}\n`;

  if (torrent.progress === 1) {
    textToSend += ctx.t("**COMPLETED**\n");
  } else {
    textToSend += formatProgress(torrent.progress);
  }

  if (!torrent.state.includes("stalled")) {
    const currentState = torrent.state.charAt(0).toUpperCase() + torrent.state.slice(1);
    const speed = convertSize(torrent.dlspeed);
    textToSend += ctx.t("**State:** {current_state} \n**Download Speed:** {download_speed}/s\n", {
      current_state: currentState,
      download_speed: speed,
    });
  }

  textToSend += ctx.t("**Size:** {torrent_size}\n", {
    torrent_size: convertSize(torrent.size),
  });

  if (!torrent.state.includes("stalled")) {
    textToSend += ctx.t("**ETA:** {torrent_eta}\n", {
      torrent_eta: convertEta(Number(torrent.eta)),
    });
  }

  if (torrent.category) {
    textToSend += ctx.t("**Category:** {torrent_category}\n", {
      torrent_category: torrent.category,
    });
  }

  const keyboard = new InlineKeyboard()
    .text(ctx.t("💾 Export torrent"), `export:${torrent.hash}`)
    .row()
    .text(ctx.t("📝 Edit Category"), `edit_torrent_cat:${torrent.hash}`)
    .row()
    .text(ctx.t("⏸ Pause"), `pause:${torrent.hash}`)
    .row()
    .text(ctx.t("▶️ Resume"), `resume:${torrent.hash}`)
    .row()
    .text(ctx.t("🗑 Delete"), `delete_one:${torrent.hash}`)
    .row()
    .text(ctx.t("🔙 Menu"), "menu:").row();

  await ctx.api.editMessageText(ctx.chat!.id, messageId, textToSend, {
    reply_markup: keyboard,
    parse_mode: 'Markdown',
  });
}
