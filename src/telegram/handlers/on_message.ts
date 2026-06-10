import { Composer } from 'grammy';
import { BotContext } from '../index';
import { sendMenu } from './common';
import { ClientRepo } from '../../client_manager';
import { join } from 'path';
import { existsSync, mkdirSync, unlinkSync } from 'fs';

export const messageComposer = new Composer<BotContext>();

async function onMagnet(ctx: BotContext, text: string, action: string) {
  const userId = ctx.from?.id;
  if (!userId) return;

  if (text.startsWith("magnet:?xt")) {
    const magnetLinks = text.split("\n").map(l => l.trim()).filter(Boolean);
    const category = action.includes('#') ? action.split('#')[1] : null;
    const cleanCategory = category === 'None' || !category ? null : category;

    try {
      const manager = ClientRepo.getClientManager(ctx.settings);
      const success = await manager.add_magnet(magnetLinks, cleanCategory);

      if (!success) {
        await ctx.reply(ctx.t("Unable to add magnet link"));
        return;
      }

      await ctx.redis.set(`action:${userId}`, '');
      await sendMenu(ctx);
    } catch (e) {
      console.error("Error adding magnet in Telegram handler:", e);
      if (e instanceof Error && e.message.includes('409')) {
        await ctx.reply("⚠️ This torrent/magnet link is already in the download list.");
      } else {
        await ctx.reply(ctx.t("Unable to add magnet link"));
      }
    }
  } else {
    await ctx.reply(ctx.t("This magnet link is invalid! Retry"));
  }
}

async function onTorrent(ctx: BotContext, fileId: string, fileName: string, action: string) {
  const userId = ctx.from?.id;
  if (!userId) return;

  if (fileName.endsWith(".torrent")) {
    const tempDir = join(import.meta.dir, '../../temp');
    if (!existsSync(tempDir)) {
      mkdirSync(tempDir, { recursive: true });
    }
    const tempFilePath = join(tempDir, `${Date.now()}_${fileName}`);

    try {
      // Get file info from Telegram
      const file = await ctx.api.getFile(fileId);
      const fileUrl = `https://api.telegram.org/file/bot${ctx.settings.telegram.bot_token}/${file.file_path}`;

      // Download file using fetch
      const res = await fetch(fileUrl);
      if (!res.ok) throw new Error(`Failed to download torrent: ${res.statusText}`);
      const buffer = await res.arrayBuffer();
      await Bun.write(tempFilePath, buffer);

      // Add to qbittorrent
      const category = action.includes('#') ? action.split('#')[1] : null;
      const cleanCategory = category === 'None' || !category ? null : category;

      const manager = ClientRepo.getClientManager(ctx.settings);
      const success = await manager.add_torrent(tempFilePath, cleanCategory);

      if (!success) {
        await ctx.reply(ctx.t("Unable to add torrent file"));
        return;
      }

      await ctx.redis.set(`action:${userId}`, '');
      await sendMenu(ctx);
    } catch (e) {
      console.error("Error downloading or adding torrent file", e);
      if (e instanceof Error && e.message.includes('409')) {
        await ctx.reply("⚠️ This torrent/magnet link is already in the download list.");
      } else {
        await ctx.reply(ctx.t("Unable to add torrent file"));
      }
    } finally {
      // Clean up temp file
      if (existsSync(tempFilePath)) {
        unlinkSync(tempFilePath);
      }
    }
  } else {
    await ctx.reply(ctx.t("This is not a torrent file! Retry"));
  }
}

async function onCategoryName(ctx: BotContext, text: string) {
  const userId = ctx.from?.id;
  if (!userId) return;

  await ctx.redis.set(`action:${userId}`, `category_dir#${text}`);
  await ctx.reply(ctx.t("Please, send the path for the category {category_name}", { category_name: text }));
}

async function onCategoryDirectory(ctx: BotContext, text: string, action: string) {
  const userId = ctx.from?.id;
  if (!userId) return;

  const categoryName = action.split('#')[1];
  const savePath = text.replace(/\\/g, ''); // strip backslashes like Python version

  const manager = ClientRepo.getClientManager(ctx.settings);

  try {
    if (action.includes('modify')) {
      await manager.edit_category(categoryName, savePath);
    } else {
      await manager.create_category(categoryName, savePath);
    }
    await ctx.redis.set(`action:${userId}`, '');
    await sendMenu(ctx);
  } catch (e) {
    console.error("Failed to modify/create category directory", e);
    await ctx.reply(ctx.t("Unable to save category path"));
  }
}

// Handler for all non-command messages
messageComposer.on(['message:text', 'message:document'], async (ctx) => {
  const userId = ctx.from?.id;
  if (!userId) return;

  // Skip commands (they start with /)
  if (ctx.message.text && ctx.message.text.startsWith('/')) {
    return;
  }

  const action = await ctx.redis.get(`action:${userId}`) || '';

  if (ctx.message.document && !action) {
    // Direct torrent upload without clicking add button
    await onTorrent(ctx, ctx.message.document.file_id, ctx.message.document.file_name || '', action);
  } else if (action.includes('magnet')) {
    await onMagnet(ctx, ctx.message.text || '', action);
  } else if (action.includes('torrent') && ctx.message.document) {
    await onTorrent(ctx, ctx.message.document.file_id, ctx.message.document.file_name || '', action);
  } else if (action === 'category_name') {
    await onCategoryName(ctx, ctx.message.text || '');
  } else if (action.includes('category_dir')) {
    await onCategoryDirectory(ctx, ctx.message.text || '', action);
  } else {
    await ctx.reply(ctx.t("The command does not exist"));
  }
});
