import { Database } from 'bun:sqlite';
import { join } from 'path';
import { existsSync, mkdirSync } from 'fs';

let dbInstance: Database | null = null;

export function getDatabasePath(): string {
  if (process.env.DATABASE_PATH) {
    return process.env.DATABASE_PATH;
  }
  const dataDir = join(import.meta.dir, '../../data');
  if (!existsSync(dataDir)) {
    mkdirSync(dataDir, { recursive: true });
  }
  return join(dataDir, 'magpie.db');
}

export function getDatabase(): Database {
  if (dbInstance) return dbInstance;

  const dbPath = getDatabasePath();
  dbInstance = new Database(dbPath);

  // Initialize tables
  dbInstance.run(`
    CREATE TABLE IF NOT EXISTS notifications (
      hash TEXT PRIMARY KEY,
      sent_at INTEGER NOT NULL
    )
  `);

  dbInstance.run(`
    CREATE TABLE IF NOT EXISTS presigned_links (
      s3_key TEXT PRIMARY KEY,
      url TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      expires_in INTEGER NOT NULL
    )
  `);

  dbInstance.run(`
    CREATE TABLE IF NOT EXISTS users (
      discord_id TEXT UNIQUE,
      telegram_id INTEGER UNIQUE,
      role TEXT NOT NULL,
      locale TEXT,
      notify INTEGER DEFAULT 1,
      notification_filter TEXT
    )
  `);

  return dbInstance;
}

export function closeDatabase(): void {
  if (dbInstance) {
    dbInstance.close();
    dbInstance = null;
  }
}

// 1. Sent Notifications
export function isNotificationSent(hash: string): boolean {
  const db = getDatabase();
  const row = db.prepare('SELECT 1 FROM notifications WHERE hash = ?').get(hash);
  return !!row;
}

export function markNotificationSent(hash: string): void {
  const db = getDatabase();
  db.prepare('INSERT OR REPLACE INTO notifications (hash, sent_at) VALUES (?, ?)')
    .run(hash, Math.floor(Date.now() / 1000));
}

// 2. Presigned Link Caching
export function getCachedPresignedUrl(key: string): string | null {
  const db = getDatabase();
  const row = db.prepare('SELECT url, created_at, expires_in FROM presigned_links WHERE s3_key = ?').get(key) as any;
  if (!row) return null;

  const now = Math.floor(Date.now() / 1000);
  const elapsed = now - row.created_at;
  const remaining = row.expires_in - elapsed;
  const percentRemaining = (remaining / row.expires_in) * 100;

  if (percentRemaining > 75) {
    console.log(`[DB] Reusing cached presigned URL for key "${key}" (${percentRemaining.toFixed(1)}% time remaining)`);
    return row.url;
  }

  return null;
}

export function cachePresignedUrl(key: string, url: string, expiresIn: number): void {
  const db = getDatabase();
  const now = Math.floor(Date.now() / 1000);
  db.prepare('INSERT OR REPLACE INTO presigned_links (s3_key, url, created_at, expires_in) VALUES (?, ?, ?, ?)')
    .run(key, url, now, expiresIn);
}

// 3. User Authorization Database
export interface DBUser {
  user_id: number; // maps to telegram_id
  discord_id: string | null;
  role: string;
  locale: string | null;
  notify: boolean;
  notification_filter: string[];
}

export function loadUsersFromDB(): DBUser[] {
  const db = getDatabase();
  const rows = db.prepare('SELECT * FROM users').all() as any[];
  return rows.map(r => ({
    user_id: r.telegram_id ? Number(r.telegram_id) : 0,
    discord_id: r.discord_id || null,
    role: r.role,
    locale: r.locale || null,
    notify: r.notify === 1,
    notification_filter: r.notification_filter ? JSON.parse(r.notification_filter) : []
  }));
}

export function saveUserToDB(user: DBUser): void {
  const db = getDatabase();
  db.prepare(`
    INSERT OR REPLACE INTO users (discord_id, telegram_id, role, locale, notify, notification_filter)
    VALUES (?, ?, ?, ?, ?, ?)
  `).run(
    user.discord_id,
    user.user_id ? user.user_id : null,
    user.role,
    user.locale,
    user.notify ? 1 : 0,
    JSON.stringify(user.notification_filter)
  );
}

export function syncUsers(yamlUsers: any[]): DBUser[] {
  const db = getDatabase();
  const existingUsers = loadUsersFromDB();

  // Begin transaction for speed
  const transaction = db.transaction((usersToInsert: any[]) => {
    for (const yu of usersToInsert) {
      const dId = yu.discord_id || null;
      const tId = yu.user_id ? Number(yu.user_id) : null;

      const dbUser = existingUsers.find(eu => 
        (dId && eu.discord_id === dId) || (tId && eu.user_id === tId)
      );

      if (!dbUser) {
        saveUserToDB({
          user_id: tId || 0,
          discord_id: dId,
          role: yu.role || 'reader',
          locale: yu.locale || null,
          notify: yu.notify !== false,
          notification_filter: yu.notification_filter || []
        });
      }
    }
  });

  transaction(yamlUsers);
  return loadUsersFromDB();
}
