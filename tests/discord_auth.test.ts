import { describe, expect, test, mock } from 'bun:test';
import { handleMessage } from '../src/discord/handlers/on_message';
import { handleInteraction } from '../src/discord/handlers/interactions';
import { Settings } from '../src/settings';
import { ChannelType, ButtonStyle } from 'discord.js';

describe('Discord Dynamic Authorization Unit Tests', () => {
  // A helper to create clean, fresh Settings instance
  const createSettings = (users: any[] = []) => {
    return new Settings({
      client: { host: 'http://localhost:8080', user: 'admin', password: 'pw' },
      telegram: { enabled: false, bot_token: '' },
      discord: { enabled: true, token: 'tok' },
      users,
      redis: { url: null }
    });
  };

  const mockManager: any = {};
  const mockClient: any = { user: { id: 'bot_id' } };

  test('Auto-authorizes first administrator if no admin is configured', async () => {
    const settings = createSettings(); // Empty users
    settings.exportSettings = mock(() => {});

    const mockMessage: any = {
      channel: { type: ChannelType.DM }, // DM
      author: { id: 'first_user_id', tag: 'AdminUser#1111' },
      content: 'Hello bot',
      mentions: { has: () => false },
      attachments: { size: 0 },
      reply: mock(async () => ({}))
    };

    await handleMessage(mockMessage, mockClient, mockManager, settings);

    // Should have replied with authorization notification
    expect(mockMessage.reply).toHaveBeenCalledWith('👑 You have been automatically authorized as the first **administrator**!');
    
    // Should have added user and exported settings
    expect(settings.users.length).toBe(1);
    expect(settings.users[0].discord_id).toBe('first_user_id');
    expect(settings.users[0].role).toBe('administrator');
    expect(settings.exportSettings).toHaveBeenCalled();
  });

  test('Provides Request Access button for unauthorized users when admin exists', async () => {
    // Admin exists
    const settings = createSettings([
      { discord_id: 'configured_admin', role: 'administrator' }
    ]);
    settings.exportSettings = mock(() => {});

    let replyOptions: any = null;
    const mockMessage: any = {
      channel: { type: ChannelType.DM }, // DM
      author: { id: 'new_user_id', tag: 'NewUser#2222' },
      content: 'Hello bot',
      mentions: { has: () => false },
      attachments: { size: 0 },
      reply: mock(async (options: any) => {
        replyOptions = options;
        return {};
      })
    };

    await handleMessage(mockMessage, mockClient, mockManager, settings);

    expect(mockMessage.reply).toHaveBeenCalled();
    expect(replyOptions).toBeDefined();
    expect(replyOptions.content).toContain('❌ You are not authorized to use this bot.');
    expect(replyOptions.components[0].components[0].data.custom_id).toBe('dc_request_access');
  });

  test('Request Access updates status and notifies configured administrators', async () => {
    const settings = createSettings([
      { discord_id: 'configured_admin', role: 'administrator' }
    ]);

    let sentMessage: any = null;
    const mockAdminUser = {
      send: mock(async (msg: any) => {
        sentMessage = msg;
        return {};
      })
    };

    const mockInteraction: any = {
      isStringSelectMenu: () => false,
      isButton: () => true,
      customId: 'dc_request_access',
      user: { id: 'requestor_id', username: 'requestor' },
      client: {
        users: {
          fetch: mock(async (id: string) => {
            if (id === 'configured_admin') return mockAdminUser;
            return null;
          })
        }
      },
      update: mock(async () => ({}))
    };

    await handleInteraction(mockInteraction, mockManager, settings);

    // Updates message state
    expect(mockInteraction.update).toHaveBeenCalled();
    expect(mockInteraction.update.mock.calls[0][0].content).toBe('⏳ Access request sent to administrators. Please wait...');

    // Sends access request to admin
    expect(mockAdminUser.send).toHaveBeenCalled();
    expect(sentMessage.content).toContain('requestor_id');
    expect(sentMessage.components[0].components[0].data.custom_id).toBe('dc_auth:approve:administrator:requestor_id:requestor');
  });

  test('Admin approval button adds user and notifies the requestor', async () => {
    const settings = createSettings([
      { discord_id: 'configured_admin', role: 'administrator' }
    ]);
    settings.exportSettings = mock(() => {});

    let targetSentMessage: any = null;
    const mockTargetUser = {
      send: mock(async (msg: any) => {
        targetSentMessage = msg;
        return {};
      })
    };

    const mockInteraction: any = {
      isStringSelectMenu: () => false,
      isButton: () => true,
      customId: 'dc_auth:approve:manager:requestor_id:requestor',
      user: { id: 'configured_admin', username: 'admin' }, // Admin interacting
      client: {
        users: {
          fetch: mock(async (id: string) => {
            if (id === 'requestor_id') return mockTargetUser;
            return null;
          })
        }
      },
      update: mock(async () => ({}))
    };

    await handleInteraction(mockInteraction, mockManager, settings);

    // Stored & persisted
    expect(settings.users.length).toBe(2);
    expect(settings.users[1].discord_id).toBe('requestor_id');
    expect(settings.users[1].role).toBe('manager');
    expect(settings.exportSettings).toHaveBeenCalled();

    // Interaction updated
    expect(mockInteraction.update).toHaveBeenCalled();
    expect(mockInteraction.update.mock.calls[0][0].content).toContain('Authorized **requestor**');

    // Notified user
    expect(mockTargetUser.send).toHaveBeenCalled();
    expect(targetSentMessage).toContain('approved');
  });

  test('Ignores messages from other bots to prevent loops', async () => {
    const settings = createSettings();
    const mockMessage: any = {
      channel: { type: ChannelType.DM },
      author: { id: 'other_bot_id', tag: 'Bot#1111', bot: true },
      content: 'Hello bot',
      mentions: { has: () => false },
      attachments: { size: 0 },
      reply: mock(async () => ({}))
    };

    await handleMessage(mockMessage, mockClient, mockManager, settings);

    // Should NOT have replied
    expect(mockMessage.reply).not.toHaveBeenCalled();
  });
});
