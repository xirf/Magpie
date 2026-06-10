import { describe, expect, test, mock, beforeEach, afterEach } from 'bun:test';

// Set environment variable for test isolation to use in-memory database
process.env.DATABASE_PATH = ':memory:';

// Mock the S3 module imports before importing the files that use them.
const mockSend = mock(async (command: any) => ({}));
const mockGetSignedUrl = mock(async (client: any, command: any, options: any) => {
  return `http://mock-presigned-url.com/${command.input.Key}?expiry=${options.expiresIn}`;
});

mock.module("@aws-sdk/client-s3", () => {
  return {
    S3Client: class {
      send = mockSend;
    },
    PutObjectCommand: class {
      constructor(public input: any) {}
    },
    GetObjectCommand: class {
      constructor(public input: any) {}
    }
  };
});

mock.module("@aws-sdk/s3-request-presigner", () => {
  return {
    getSignedUrl: mockGetSignedUrl
  };
});

// Import client repo and override
import { ClientRepo } from '../src/client_manager';
import { Settings } from '../src/settings';
import { generatePresignedUrl, uploadFolderOrFileToS3, getS3Client } from '../src/utils/s3';
import { torrentFinished } from '../src/tasks';
import { RedisEmulator } from '../src/redis_helper';
import { handleInteraction } from '../src/discord/handlers/interactions';
import { handleCommand } from '../src/discord/handlers/commands';
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';
import { closeDatabase, getDatabase } from '../src/utils/db';

const mockManager: any = {
  get_torrents: mock(async (hash: string | null, filter: string | null) => []),
  get_torrent: mock(async (hash: string) => null),
  delete_one_data: mock(async (hash: string) => {}),
  pause: mock(async (hash: string) => {})
};

ClientRepo.getClientManager = () => mockManager;

describe('S3 Integration Tests', () => {
  const tempDir = join(import.meta.dir, 'temp_s3_test');
  const tempSubDir = join(tempDir, 'nested');
  const tempFile1 = join(tempDir, 'file1.txt');
  const tempFile2 = join(tempSubDir, 'file2.bin');

  beforeEach(() => {
    mockSend.mockClear();
    mockGetSignedUrl.mockClear();
    mockManager.get_torrents.mockClear();
    mockManager.get_torrent.mockClear();
    mockManager.delete_one_data.mockClear();
    mockManager.pause.mockClear();

    // Reset DB
    closeDatabase();
    getDatabase();

    // Create temp files and directories
    if (existsSync(tempDir)) {
      rmSync(tempDir, { recursive: true, force: true });
    }
    mkdirSync(tempDir, { recursive: true });
    mkdirSync(tempSubDir, { recursive: true });
    writeFileSync(tempFile1, 'hello file1'); // size 11
    writeFileSync(tempFile2, 'hello file2 nested!'); // size 19
  });

  afterEach(() => {
    closeDatabase();
    if (existsSync(tempDir)) {
      rmSync(tempDir, { recursive: true, force: true });
    }
  });

  const createSettings = (overrides: any = {}) => {
    const { seed_after_download, users, ...s3Overrides } = overrides;
    return new Settings({
      client: { host: 'http://localhost:8080', user: 'admin', password: 'pw' },
      telegram: { enabled: true, bot_token: 'tg-token' },
      discord: { enabled: true, token: 'dc-token' },
      users: users || [
        {
          user_id: 12345,
          discord_id: 'discord-user-id',
          role: 'administrator',
          locale: 'en',
          notify: true,
          notification_filter: []
        }
      ],
      redis: { url: null },
      s3: {
        enabled: true,
        endpoint: 'http://localhost:9000',
        access_key: 'test-access',
        secret_key: 'test-secret',
        bucket: 'test-bucket',
        region: 'us-east-1',
        link_expiry: 1800,
        mode: 'mount',
        ...s3Overrides
      },
      seed_after_download
    });
  };

  test('getS3Client instantiates with correct config', () => {
    const settings = createSettings();
    const client = getS3Client(settings);
    expect(client).toBeDefined();
  });

  test('generatePresignedUrl creates a signed link', async () => {
    const settings = createSettings();
    const url = await generatePresignedUrl(settings, 'test-key.mp4');
    expect(url).toBe('http://mock-presigned-url.com/test-key.mp4?expiry=1800');
    expect(mockGetSignedUrl).toHaveBeenCalled();
  });

  test('uploadFolderOrFileToS3 uploads a single file', async () => {
    const settings = createSettings();
    await uploadFolderOrFileToS3(settings, tempFile1, 'uploads');
    
    expect(mockSend).toHaveBeenCalled();
    const calls = mockSend.mock.calls;
    expect(calls.length).toBe(1);
    
    const command = calls[0][0];
    expect(command.input.Bucket).toBe('test-bucket');
    expect(command.input.Key).toBe('uploads/file1.txt');
    expect(command.input.Body.toString()).toBe('hello file1');
  });

  test('uploadFolderOrFileToS3 uploads a directory recursively', async () => {
    const settings = createSettings();
    await uploadFolderOrFileToS3(settings, tempDir, 'folder-upload');

    expect(mockSend).toHaveBeenCalled();
    const calls = mockSend.mock.calls;
    expect(calls.length).toBe(2);

    const keys = calls.map(c => c[0].input.Key);
    expect(keys).toContain('folder-upload/temp_s3_test/file1.txt');
    expect(keys).toContain('folder-upload/temp_s3_test/nested/file2.bin');
  });

  test('torrentFinished in mount mode generates signed link, notifies users, does not delete local files', async () => {
    const settings = createSettings({ mode: 'mount' });
    const redis = new RedisEmulator() as any;
    
    mockManager.get_torrents.mockImplementation(async () => [
      {
        hash: 'test-hash',
        name: 'temp_s3_test',
        progress: 1.0,
        dlspeed: 0,
        state: 'completed',
        size: 30,
        eta: 0,
        category: null,
        save_path: join(tempDir, '..'),
        content_path: tempDir
      }
    ]);

    const mockTelegramBot: any = {
      api: {
        sendMessage: mock(async () => ({}))
      }
    };

    const mockDiscordUser = {
      send: mock(async () => ({}))
    };
    const mockDiscordClient: any = {
      users: {
        fetch: mock(async (id: string) => {
          if (id === 'discord-user-id') return mockDiscordUser;
          return null;
        })
      }
    };

    await torrentFinished(mockTelegramBot, mockDiscordClient, redis, settings, mockManager);

    // Verify S3 link generated for the largest file (file2.bin in temp_s3_test directory)
    expect(mockGetSignedUrl).toHaveBeenCalled();
    const commandCalled = mockGetSignedUrl.mock.calls[0][1];
    expect(commandCalled.input.Key).toBe('temp_s3_test/nested/file2.bin');

    // Telegram notification check
    expect(mockTelegramBot.api.sendMessage).toHaveBeenCalled();
    const tgMsg = mockTelegramBot.api.sendMessage.mock.calls[0][1];
    expect(tgMsg).toContain('has finished downloading');

    // Discord notification check (should have sent with buttons)
    expect(mockDiscordClient.users.fetch).toHaveBeenCalledWith('discord-user-id');
    expect(mockDiscordUser.send).toHaveBeenCalled();
    const dcMsg = (mockDiscordUser.send.mock.calls as any[])[0][0];
    expect(dcMsg?.content).toContain('has finished downloading');
    expect(dcMsg?.components[0].components[0].data.label).toBe('Download');
    expect(dcMsg?.components[0].components[0].data.url).toContain('temp_s3_test/nested/file2.bin');

    // Local files should NOT be deleted
    expect(mockManager.delete_one_data).not.toHaveBeenCalled();
    expect(existsSync(tempFile1)).toBe(true);
    expect(existsSync(tempFile2)).toBe(true);
  });

  test('torrentFinished in upload mode uploads to S3, deletes local data, and notifies users', async () => {
    const settings = createSettings({ mode: 'upload' });
    const redis = new RedisEmulator() as any;

    mockManager.get_torrents.mockImplementation(async () => [
      {
        hash: 'test-hash',
        name: 'temp_s3_test',
        progress: 1.0,
        dlspeed: 0,
        state: 'completed',
        size: 30,
        eta: 0,
        category: null,
        save_path: join(tempDir, '..'),
        content_path: tempDir
      }
    ]);

    const mockTelegramBot: any = {
      api: {
        sendMessage: mock(async () => ({}))
      }
    };

    const mockDiscordUser = {
      send: mock(async () => ({}))
    };
    const mockDiscordClient: any = {
      users: {
        fetch: mock(async () => mockDiscordUser)
      }
    };

    await torrentFinished(mockTelegramBot, mockDiscordClient, redis, settings, mockManager);

    // Verify upload called
    expect(mockSend).toHaveBeenCalled();
    const uploadedKeys = mockSend.mock.calls.map(c => c[0].input.Key);
    expect(uploadedKeys).toContain('temp_s3_test/file1.txt');
    expect(uploadedKeys).toContain('temp_s3_test/nested/file2.bin');

    // Verify signed link generated
    expect(mockGetSignedUrl).toHaveBeenCalled();

    // Verify local data deleted
    expect(mockManager.delete_one_data).toHaveBeenCalledWith('test-hash');
  });

  test('handleGetLink generates link and responds to interaction', async () => {
    const settings = createSettings();
    mockManager.get_torrent.mockImplementation(async (hash: string) => ({
      hash: 'test-hash',
      name: 'temp_s3_test',
      progress: 1.0,
      dlspeed: 0,
      state: 'completed',
      size: 30,
      eta: 0,
      category: null,
      save_path: join(tempDir, '..'),
      content_path: tempDir
    }));

    let replyResult: any = null;
    const mockInteraction: any = {
      isStringSelectMenu: () => false,
      isButton: () => true,
      customId: 'dc_get_link:test-hash',
      user: { id: 'discord-user-id', username: 'User' },
      deferReply: mock(async () => ({})),
      editReply: mock(async (opts: any) => {
        replyResult = opts;
        return {};
      })
    };

    await handleInteraction(mockInteraction, mockManager, settings);

    expect(mockInteraction.deferReply).toHaveBeenCalledWith({ ephemeral: true });
    expect(mockInteraction.editReply).toHaveBeenCalled();
    expect(replyResult.content).toContain('temporary download link');
    expect(replyResult.components[0].components[0].data.label).toBe('Download');
    expect(replyResult.components[0].components[0].data.url).toContain('temp_s3_test/nested/file2.bin');
  });

  test('torrentFinished respects seed_after_download policy', async () => {
    const redis = new RedisEmulator() as any;
    const mockTelegramBot: any = { api: { sendMessage: mock(async () => ({})) } };
    const mockDiscordClient: any = { users: { fetch: mock(async () => ({ send: mock(async () => ({})) })) } };

    const completedTorrent = {
      hash: 'test-hash-policy',
      name: 'temp_s3_test',
      progress: 1.0,
      dlspeed: 0,
      state: 'completed',
      size: 30,
      eta: 0,
      category: null,
      save_path: join(tempDir, '..'),
      content_path: tempDir
    };

    // Test ALWAYS policy
    mockManager.get_torrents.mockImplementation(async () => [completedTorrent]);
    const settingsAlways = createSettings({ seed_after_download: 'always' });
    await torrentFinished(mockTelegramBot, mockDiscordClient, redis, settingsAlways, mockManager);
    expect(mockManager.pause).not.toHaveBeenCalled();

    // Test NEVER policy
    await redis.delete('test-hash-policy');
    getDatabase().run("DELETE FROM notifications WHERE hash = 'test-hash-policy'");
    const settingsNever = createSettings({ seed_after_download: 'never' });
    await torrentFinished(mockTelegramBot, mockDiscordClient, redis, settingsNever, mockManager);
    expect(mockManager.pause).toHaveBeenCalledWith('test-hash-policy');

    // Test ADMIN_ONLY policy with admin user
    mockManager.pause.mockClear();
    await redis.delete('test-hash-policy');
    getDatabase().run("DELETE FROM notifications WHERE hash = 'test-hash-policy'");
    const settingsAdminOnlyWithAdmin = createSettings({ seed_after_download: 'admin_only' });
    // settings has an administrator in its users array by default
    await torrentFinished(mockTelegramBot, mockDiscordClient, redis, settingsAdminOnlyWithAdmin, mockManager);
    expect(mockManager.pause).not.toHaveBeenCalled();

    // Test ADMIN_ONLY policy with no admin user (only manager/reader)
    mockManager.pause.mockClear();
    await redis.delete('test-hash-policy');
    getDatabase().run("DELETE FROM notifications WHERE hash = 'test-hash-policy'");
    const settingsAdminOnlyNoAdmin = createSettings({ seed_after_download: 'admin_only' });
    settingsAdminOnlyNoAdmin.users = [
      {
        user_id: 12345,
        discord_id: 'discord-user-id',
        role: 'manager',
        locale: 'en',
        notify: true,
        notification_filter: []
      }
    ];
    await torrentFinished(mockTelegramBot, mockDiscordClient, redis, settingsAdminOnlyNoAdmin, mockManager);
    expect(mockManager.pause).toHaveBeenCalledWith('test-hash-policy');
  });

  test('handleCommand seeding updates settings if admin, fails if reader/manager', async () => {
    const settings = createSettings();
    settings.exportSettings = mock(() => {}) as any;

    // 1. Admin test
    const adminUser = { role: 'administrator' } as any;
    let replyContent = '';
    const mockInteractionAdmin: any = {
      commandName: 'seeding',
      options: {
        getString: mock((name: string) => 'never')
      },
      reply: mock(async (opts: any) => {
        replyContent = opts.content || opts;
        return {};
      })
    };

    await handleCommand(mockInteractionAdmin, mockManager, adminUser, settings);
    expect(settings.seed_after_download).toBe('never');
    expect(settings.exportSettings).toHaveBeenCalled();
    expect(replyContent).toContain('Seeding policy updated');

    // 2. Reader test (should fail)
    (settings.exportSettings as any).mockClear();
    const readerUser = { role: 'reader' } as any;
    const mockInteractionReader: any = {
      commandName: 'seeding',
      options: {
        getString: mock(() => 'always')
      },
      reply: mock(async () => ({}))
    };

    await handleCommand(mockInteractionReader, mockManager, readerUser, settings);
    expect(mockInteractionReader.reply).toHaveBeenCalled();
    expect(mockInteractionReader.reply.mock.calls[0][0].content).toContain('not authorized');
    expect(settings.exportSettings).not.toHaveBeenCalled();
  });
});
