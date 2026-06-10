export function convertSize(sizeBytes: number): string {
  if (sizeBytes === 0) return "0 B";
  const sizeNames = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
  const i = Math.floor(Math.log(sizeBytes) / Math.log(1024));
  const p = Math.pow(1024, i);
  const s = Math.round((sizeBytes / p) * 100) / 100;
  return `${s} ${sizeNames[i]}`;
}

export function convertEta(seconds: number): string {
  if (seconds === 8640000 || seconds < 0) return '∞';
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;

  const pad = (num: number) => String(num).padStart(2, '0');
  const timeStr = `${pad(hours)}:${pad(minutes)}:${pad(secs)}`;
  return days > 0 ? `${days} day${days > 1 ? 's' : ''}, ${timeStr}` : timeStr;
}

export function formatProgress(progress: number, width: number = 20): string {
  progress = Math.max(0, Math.min(progress, 1));
  const filled = Math.floor(progress * width);
  const bar = "█".repeat(filled) + "░".repeat(width - filled);
  const percent = Math.floor(progress * 100);
  return `${String(percent).padStart(3, ' ')}%|${bar}|\n`;
}

export function escapeMarkdown(text: string): string {
  return text.replace(/([_*\[\]()~`>#+\-=|{}.!])/g, '\\$1');
}
