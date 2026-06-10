import { Composer } from 'grammy';
import { BotContext } from '../index';
import { sendMenu } from './common';
import { convertSize } from '../../utils';
import * as si from 'systeminformation';

export const commandsComposer = new Composer<BotContext>();

commandsComposer.command('start', async (ctx) => {
  const userId = ctx.from?.id;
  if (!userId) return;
  await ctx.redis.set(`action:${userId}`, '');
  await sendMenu(ctx);
});

commandsComposer.command('stats', async (ctx) => {
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

  const statsText = ctx.t(
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

  await ctx.reply(statsText, { parse_mode: 'Markdown' });
});
