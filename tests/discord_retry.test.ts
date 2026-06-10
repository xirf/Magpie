import { describe, expect, test, mock } from 'bun:test';
import { handleMessage } from '../src/discord/handlers/on_message';
import { handleInteraction } from '../src/discord/handlers/interactions';

describe('Discord Retry Button Unit Tests', () => {
  // Mock Settings
  const mockSettings: any = {
    users: [
      {
        discord_id: '123456789',
        role: 'administrator',
        locale: 'en'
      }
    ]
  };

  // Mock QBittorrentManager
  const mockManager: any = {
    add_magnet: mock(async (magnet: string) => {
      if (magnet.includes('fail')) {
        throw new Error('qBittorrent API request failed (500): Internal Server Error');
      }
      if (magnet.includes('conflict')) {
        throw new Error('qBittorrent API request failed (409): Conflict');
      }
      return true;
    }),
    add_torrent: mock(async (path: string) => {
      if (path.includes('fail')) {
        throw new Error('qBittorrent API request failed (500): Internal Server Error');
      }
      return true;
    })
  };

  // Mock Client
  const mockClient: any = {
    user: { id: 'bot_id' }
  };

  test('handleMessage adds retry button on magnet add failure', async () => {
    let replyOptions: any = null;

    const mockMessage: any = {
      channel: {
        type: 0, // Text channel
        sendTyping: mock(() => {})
      },
      author: {
        id: '123456789',
        tag: 'User#1234'
      },
      content: 'magnet:?xt=urn:btih:fail_magnet_link',
      mentions: {
        has: () => true
      },
      reply: mock(async (options: any) => {
        replyOptions = options;
        return {};
      })
    };

    await handleMessage(mockMessage, mockClient, mockManager, mockSettings);

    expect(mockMessage.reply).toHaveBeenCalled();
    expect(replyOptions).toBeDefined();
    expect(replyOptions.content.includes('❌ Error: qBittorrent API request failed (500): Internal Server Error')).toBe(true);
    expect(replyOptions.components).toBeDefined();
    expect(replyOptions.components.length).toBe(1);
    
    const row = replyOptions.components[0];
    expect(row.components).toBeDefined();
    expect(row.components.length).toBe(1);
    expect(row.components[0].data.custom_id).toBe('dc_retry');
  });

  test('handleMessage adds retry button on magnet add 409 conflict', async () => {
    let replyOptions: any = null;

    const mockMessage: any = {
      channel: {
        type: 0,
        sendTyping: mock(() => {})
      },
      author: {
        id: '123456789',
        tag: 'User#1234'
      },
      content: 'magnet:?xt=urn:btih:conflict_magnet_link',
      mentions: {
        has: () => true
      },
      reply: mock(async (options: any) => {
        replyOptions = options;
        return {};
      })
    };

    await handleMessage(mockMessage, mockClient, mockManager, mockSettings);

    expect(mockMessage.reply).toHaveBeenCalled();
    expect(replyOptions).toBeDefined();
    expect(replyOptions.content.includes('⚠️ This torrent/magnet link is already in the download list.')).toBe(true);
    expect(replyOptions.components).toBeDefined();
    expect(replyOptions.components.length).toBe(1);
  });

  test('handleInteraction dc_retry successfully retries parsing and adding magnet', async () => {
    let updatedOptions: any = null;
    let editReplyOptions: any = null;

    const mockOriginalMessage: any = {
      content: 'magnet:?xt=urn:btih:valid_magnet_link',
      attachments: new Map()
    };

    const mockChannel: any = {
      messages: {
        fetch: mock(async () => mockOriginalMessage)
      }
    };

    const mockInteraction: any = {
      isStringSelectMenu: () => false,
      isButton: () => true,
      customId: 'dc_retry',
      channelId: 'channel_123',
      channel: mockChannel,
      client: {
        channels: {
          fetch: mock(async () => mockChannel)
        }
      },
      message: {
        content: '❌ Error: qBittorrent API request failed (500)',
        reference: {
          messageId: 'original_msg_123'
        }
      },
      update: mock(async (options: any) => {
        updatedOptions = options;
        return {};
      }),
      editReply: mock(async (options: any) => {
        editReplyOptions = options;
        return {};
      })
    };

    await handleInteraction(mockInteraction, mockManager, mockSettings.users[0]);

    expect(mockInteraction.update).toHaveBeenCalled();
    expect(updatedOptions.content).toBe('⏳ Retrying...');
    expect(updatedOptions.components[0].components[0].data.disabled).toBe(true);

    expect(mockChannel.messages.fetch).toHaveBeenCalledWith('original_msg_123');
    expect(mockManager.add_magnet).toHaveBeenCalledWith('magnet:?xt=urn:btih:valid_magnet_link');
    
    expect(mockInteraction.editReply).toHaveBeenCalled();
    expect(editReplyOptions.content).toBe('✅ Magnet link added successfully!');
    expect(editReplyOptions.components.length).toBe(0);
  });
});
