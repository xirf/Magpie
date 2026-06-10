import { describe, expect, test } from 'bun:test';
import { Settings } from '../src/settings';

describe('Settings', () => {
  test('getDefaultSettings returns correct structure', () => {
    const s = Settings.getDefaultSettings();
    expect(s.client.type).toBe('qbittorrent');
    expect(s.client.host).toBe('http://localhost:8080');
    expect(s.telegram.bot_token).toBe('PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE');
    expect(s.telegram.proxy).toBeNull();
    expect(s.discord.enabled).toBe(false);
    expect(s.discord.token).toBe('PUT_YOUR_DISCORD_BOT_TOKEN_HERE');
    expect(s.users.length).toBe(1);
    expect(s.users[0].user_id).toBe(123456789);
    expect(s.users[0].discord_id).toBeNull();
    expect(s.users[0].role).toBe('administrator');
    expect(s.redis.url).toBeNull();
  });

  test('updateFrom validates and updates fields', () => {
    const s = Settings.getDefaultSettings();
    s.updateFrom({
      client: {
        type: 'qbittorrent',
        host: 'https://qbt.local:443',
        user: 'custom_user',
        password: 'custom_password'
      },
      telegram: {
        bot_token: '123456:ABC-DEF',
        proxy: {
          scheme: 'socks5',
          hostname: '127.0.0.1',
          port: 1080,
          username: 'proxyuser',
          password: 'proxypassword'
        }
      },
      discord: {
        enabled: true,
        token: '777888:XYZ-123'
      },
      users: [
        {
          user_id: 111222333,
          discord_id: '999888777666',
          role: 'manager',
          locale: 'it',
          notify: false,
          notification_filter: ['movies']
        }
      ],
      redis: {
        url: 'redis://localhost:6379'
      }
    });

    expect(s.client.host).toBe('https://qbt.local:443');
    expect(s.client.user).toBe('custom_user');
    expect(s.client.password).toBe('custom_password');
    expect(s.telegram.bot_token).toBe('123456:ABC-DEF');
    expect(s.telegram.proxy?.scheme).toBe('socks5');
    expect(s.telegram.proxy?.hostname).toBe('127.0.0.1');
    expect(s.telegram.proxy?.port).toBe(1080);
    expect(s.telegram.proxy?.username).toBe('proxyuser');
    expect(s.telegram.proxy?.password).toBe('proxypassword');
    expect(s.discord.enabled).toBe(true);
    expect(s.discord.token).toBe('777888:XYZ-123');
    expect(s.users.length).toBe(1);
    expect(s.users[0].user_id).toBe(111222333);
    expect(s.users[0].discord_id).toBe('999888777666');
    expect(s.users[0].role).toBe('manager');
    expect(s.users[0].locale).toBe('it');
    expect(s.users[0].notify).toBe(false);
    expect(s.users[0].notification_filter).toEqual(['movies']);
    expect(s.redis.url).toBe('redis://localhost:6379');
  });

  test('updateFrom throws error if client user is empty', () => {
    const s = Settings.getDefaultSettings();
    expect(() => {
      s.updateFrom({
        client: {
          user: ' '
        }
      });
    }).toThrow('User cannot be empty');
  });

  test('updateFrom throws error if client password is empty', () => {
    const s = Settings.getDefaultSettings();
    expect(() => {
      s.updateFrom({
        client: {
          password: ' '
        }
      });
    }).toThrow('Password cannot be empty');
  });

  test('updateFrom throws error if bot token is empty', () => {
    const s = Settings.getDefaultSettings();
    expect(() => {
      s.updateFrom({
        telegram: {
          enabled: true,
          bot_token: ' '
        }
      });
    }).toThrow('Telegram Bot token cannot be empty when Telegram is enabled');
  });

  test('updateFrom does not throw error if bot token is empty but Telegram is disabled', () => {
    const s = Settings.getDefaultSettings();
    s.updateFrom({
      telegram: {
        enabled: false,
        bot_token: ' '
      }
    });
    expect(s.telegram.enabled).toBe(false);
  });

  test('getConnectionString yields correct objects', () => {
    const s = Settings.getDefaultSettings();
    const conn = Settings.getClientConnectionString(s.client);
    expect(conn.host).toBe('http://localhost:8080');
    expect(conn.username).toBe('admin');
    expect(conn.password).toBe('adminadmin');
  });

  test('getProxyConnectionString generates URL correctly', () => {
    const connStr = Settings.getProxyConnectionString({
      scheme: 'http',
      hostname: 'proxy.server',
      port: 3128,
      username: 'user',
      password: 'pwd'
    });
    expect(connStr).toBe('http://user:pwd@proxy.server:3128');

    const connStrNoAuth = Settings.getProxyConnectionString({
      scheme: 'socks5',
      hostname: '192.168.1.1',
      port: 1080
    });
    expect(connStrNoAuth).toBe('socks5://192.168.1.1:1080');
  });

  test('updateFrom handles single object for users instead of array', () => {
    const s = Settings.getDefaultSettings();
    s.updateFrom({
      users: {
        user_id: 99999,
        role: 'manager',
        notify: true
      }
    });
    expect(s.users.length).toBe(1);
    expect(s.users[0].user_id).toBe(99999);
    expect(s.users[0].role).toBe('manager');
  });

  test('seed_after_download defaults to always', () => {
    const s = Settings.getDefaultSettings();
    expect(s.seed_after_download).toBe('always');
  });

  test('updateFrom parses seed_after_download values correctly', () => {
    const s = Settings.getDefaultSettings();
    
    s.updateFrom({ seed_after_download: 'never' });
    expect(s.seed_after_download).toBe('never');

    s.updateFrom({ seed_after_download: false });
    expect(s.seed_after_download).toBe('never');

    s.updateFrom({ seed_after_download: 'admin_only' });
    expect(s.seed_after_download).toBe('admin_only');

    s.updateFrom({ seed_after_download: 'always' });
    expect(s.seed_after_download).toBe('always');

    s.updateFrom({ seed_after_download: true });
    expect(s.seed_after_download).toBe('always');
  });
});
