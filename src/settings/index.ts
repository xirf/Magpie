import { readFileSync, writeFileSync, existsSync, unlinkSync, mkdirSync } from 'fs';
import { join } from 'path';
import * as yaml from 'yaml';

export type ClientType = 'qbittorrent';
export type UserRole = 'reader' | 'manager' | 'administrator';
export type ProxyScheme = 'socks4' | 'socks5' | 'http';

export interface ClientSettings {
  type: ClientType;
  host: string;
  user: string;
  password: string;
}

export interface TelegramProxySettings {
  scheme: ProxyScheme;
  hostname: string;
  port: number;
  username?: string;
  password?: string;
}

export interface TelegramSettings {
  enabled: boolean;
  bot_token: string;
  proxy?: TelegramProxySettings | null;
}

export interface UserSettings {
  user_id: number;
  discord_id?: string | null;
  role: UserRole;
  locale?: string | null;
  notify?: boolean | null;
  notification_filter?: string[] | null;
}

export interface RedisSettings {
  url?: string | null;
}

export interface S3Settings {
  enabled: boolean;
  endpoint?: string | null;
  access_key?: string | null;
  secret_key?: string | null;
  bucket?: string | null;
  region?: string | null;
  link_expiry?: number | null;
  mode?: 'mount' | 'upload' | null;
}

export interface DiscordSettings {
  enabled: boolean;
  token?: string | null;
}

export class Settings {
  client!: ClientSettings;
  telegram!: TelegramSettings;
  discord!: DiscordSettings;
  users!: UserSettings[];
  redis!: RedisSettings;
  s3!: S3Settings;
  seed_after_download!: 'always' | 'never' | 'admin_only';

  constructor(data: any) {
    this.updateFrom(data);
  }

  updateFrom(newSettings: any) {
    const user = newSettings.client?.user;
    if (user !== undefined && (!user || !user.trim())) {
      throw new Error('User cannot be empty');
    }
    const password = newSettings.client?.password;
    if (password !== undefined && (!password || !password.trim())) {
      throw new Error('Password cannot be empty');
    }
    const bot_token = newSettings.telegram?.bot_token;
    const tgEnabled = newSettings.telegram?.enabled !== false;
    if (tgEnabled && bot_token !== undefined && (!bot_token || !bot_token.trim())) {
      throw new Error('Telegram Bot token cannot be empty when Telegram is enabled');
    }

    const discordToken = newSettings.discord?.token;
    const discordEnabled = !!newSettings.discord?.enabled;
    if (discordEnabled && discordToken !== undefined && (!discordToken || !discordToken.trim())) {
      throw new Error('Discord Bot token cannot be empty when Discord is enabled');
    }

    this.client = {
      type: newSettings.client?.type ?? 'qbittorrent',
      host: newSettings.client?.host ?? 'http://localhost:8080',
      user: newSettings.client?.user ?? 'admin',
      password: newSettings.client?.password ?? 'adminadmin',
    };

    this.telegram = {
      enabled: tgEnabled,
      bot_token: newSettings.telegram?.bot_token ?? 'PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE',
      proxy: newSettings.telegram?.proxy ? {
        scheme: newSettings.telegram.proxy.scheme ?? 'http',
        hostname: newSettings.telegram.proxy.hostname ?? '',
        port: Number(newSettings.telegram.proxy.port ?? 80),
        username: newSettings.telegram.proxy.username || undefined,
        password: newSettings.telegram.proxy.password || undefined,
      } : null,
    };

    this.discord = {
      enabled: !!newSettings.discord?.enabled,
      token: newSettings.discord?.token || null,
    };

    let rawUsers = newSettings.users;
    if (rawUsers && !Array.isArray(rawUsers)) {
      rawUsers = [rawUsers];
    }
    const yamlUsers = (rawUsers ?? []).map((u: any) => ({
      user_id: u.user_id !== undefined && u.user_id !== null ? Number(u.user_id) : 0,
      discord_id: u.discord_id !== undefined && u.discord_id !== null ? String(u.discord_id) : null,
      role: u.role ?? 'reader',
      locale: u.locale || null,
      notify: u.notify !== false,
      notification_filter: Array.isArray(u.notification_filter) ? u.notification_filter : [],
    }));

    try {
      const { syncUsers } = require('../utils/db');
      this.users = syncUsers(yamlUsers);
    } catch (e) {
      console.error("Failed to sync users with SQLite database, falling back to config users:", e);
      this.users = yamlUsers;
    }

    this.redis = {
      url: newSettings.redis?.url || null,
    };

    this.s3 = {
      enabled: !!newSettings.s3?.enabled,
      endpoint: newSettings.s3?.endpoint || null,
      access_key: newSettings.s3?.access_key || null,
      secret_key: newSettings.s3?.secret_key || null,
      bucket: newSettings.s3?.bucket || null,
      region: newSettings.s3?.region || null,
      link_expiry: newSettings.s3?.link_expiry !== undefined && newSettings.s3?.link_expiry !== null ? Number(newSettings.s3.link_expiry) : 3600,
      mode: newSettings.s3?.mode || 'mount',
    };

    const rawSeed = newSettings.seed_after_download;
    if (rawSeed === false || rawSeed === 'never') {
      this.seed_after_download = 'never';
    } else if (rawSeed === 'admin_only') {
      this.seed_after_download = 'admin_only';
    } else {
      this.seed_after_download = 'always';
    }
  }

  exportSettings() {
    const dataDir = join(import.meta.dir, '../../data');
    if (!existsSync(dataDir)) {
      mkdirSync(dataDir, { recursive: true });
    }
    const ymlPath = join(dataDir, 'config.yml');
    const data = {
      client: this.client,
      telegram: this.telegram,
      discord: this.discord,
      users: this.users,
      redis: this.redis,
      s3: this.s3,
      seed_after_download: this.seed_after_download,
    };
    writeFileSync(ymlPath, yaml.stringify(data, { indent: 2 }), 'utf-8');
  }

  static getClientConnectionString(client: ClientSettings) {
    return {
      host: client.host,
      username: client.user,
      password: client.password,
    };
  }

  static getProxyConnectionString(proxy: TelegramProxySettings): string {
    const auth = proxy.username && proxy.password ? `${proxy.username}:${proxy.password}@` : '';
    return `${proxy.scheme}://${auth}${proxy.hostname}:${proxy.port}`;
  }

  static loadSettings(): Settings {
    const dataDir = join(import.meta.dir, '../../data');
    const ymlPath = join(dataDir, 'config.yml');
    const jsonPath = join(dataDir, 'config.json');

    if (!existsSync(dataDir)) {
      mkdirSync(dataDir, { recursive: true });
    }

    // If config.yml does not exist
    if (!existsSync(ymlPath)) {
      // If config.json exists, migrate it
      if (existsSync(jsonPath)) {
        try {
          const raw = readFileSync(jsonPath, 'utf-8');
          const jsonData = JSON.parse(raw);
          // Migrate schema
          jsonData.redis = { url: null };
          if (Array.isArray(jsonData.users)) {
            for (const user of jsonData.users) {
              user.notification_filter = user.notification_filter || [];
            }
          }
          const settings = new Settings(jsonData);
          settings.exportSettings();
          unlinkSync(jsonPath);
          return settings;
        } catch (e) {
          console.error("Failed to migrate config.json", e);
        }
      }

      // Generate defaults
      const defaults = Settings.getDefaultSettings();
      defaults.exportSettings();
      return defaults;
    }

    // Load config.yml
    try {
      const raw = readFileSync(ymlPath, 'utf-8');
      const loadedData = yaml.parse(raw);
      return new Settings(loadedData);
    } catch (e) {
      console.error("Failed to parse config.yml, loading defaults", e);
      return Settings.getDefaultSettings();
    }
  }

  static getDefaultSettings(): Settings {
    return new Settings({
      client: {
        type: 'qbittorrent',
        host: 'http://localhost:8080',
        user: 'admin',
        password: 'adminadmin'
      },
      telegram: {
        enabled: true,
        bot_token: 'PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE',
        proxy: null
      },
      discord: {
        enabled: false,
        token: 'PUT_YOUR_DISCORD_BOT_TOKEN_HERE'
      },
      users: [
        {
          user_id: 123456789,
          discord_id: null,
          role: 'administrator',
          notify: true,
          notification_filter: []
        }
      ],
      redis: {
        url: null
      },
      s3: {
        enabled: false,
        endpoint: null,
        access_key: null,
        secret_key: null,
        bucket: null,
        region: null,
        link_expiry: 3600,
        mode: 'mount'
      },
      seed_after_download: 'always'
    });
  }
}
