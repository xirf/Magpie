import { describe, expect, test, mock } from 'bun:test';
import { showTorrentList } from '../src/discord/handlers/common';
import { handleInteraction } from '../src/discord/handlers/interactions';
import { ButtonStyle } from 'discord.js';

describe('Discord Status Filtering Menu Unit Tests', () => {
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
    get_torrents: mock(async (hash: string | null, filter: string | null) => {
      // Return mock torrent list based on filter
      const allTorrents = [
        { hash: 'h1', name: 'Torrent 1', progress: 0.5, dlspeed: 100, state: 'downloading', size: 1000, eta: 10, category: null },
        { hash: 'h2', name: 'Torrent 2', progress: 1.0, dlspeed: 0, state: 'completed', size: 2000, eta: 0, category: null },
        { hash: 'h3', name: 'Torrent 3', progress: 0.2, dlspeed: 0, state: 'paused', size: 3000, eta: 0, category: null }
      ];
      if (!filter || filter === 'all') {
        return allTorrents;
      }
      return allTorrents.filter(t => t.state === filter);
    })
  };

  test('showTorrentList displays all filters and highlights active one', async () => {
    let replyOptions: any = null;

    const mockMessage: any = {
      isButton: () => false,
      isStringSelectMenu: () => false,
      reply: mock(async (options: any) => {
        replyOptions = options;
        return {};
      })
    };

    // 1. Render list with no status filter (defaults to All active)
    await showTorrentList(mockMessage, mockManager, mockSettings.users[0]);
    expect(mockManager.get_torrents).toHaveBeenCalledWith(null, null);
    expect(replyOptions).toBeDefined();
    
    // Components should contain [SelectRow, FilterRow]
    expect(replyOptions.components.length).toBe(2);
    const filterRow = replyOptions.components[1];
    
    // Check buttons
    expect(filterRow.components.length).toBe(4);
    
    // Buttons are All, Downloading, Completed, Paused
    const [allBtn, dlBtn, compBtn, pauseBtn] = filterRow.components;
    
    expect(allBtn.data.custom_id).toBe('dc_status:all');
    expect(allBtn.data.style).toBe(ButtonStyle.Primary); // Active
    
    expect(dlBtn.data.custom_id).toBe('dc_status:downloading');
    expect(dlBtn.data.style).toBe(ButtonStyle.Secondary);
    
    expect(compBtn.data.custom_id).toBe('dc_status:completed');
    expect(compBtn.data.style).toBe(ButtonStyle.Secondary);
    
    expect(pauseBtn.data.custom_id).toBe('dc_status:paused');
    expect(pauseBtn.data.style).toBe(ButtonStyle.Secondary);
  });

  test('showTorrentList with active filter', async () => {
    let replyOptions: any = null;

    const mockMessage: any = {
      isButton: () => false,
      isStringSelectMenu: () => false,
      reply: mock(async (options: any) => {
        replyOptions = options;
        return {};
      })
    };

    // Render list with 'downloading' active filter
    await showTorrentList(mockMessage, mockManager, mockSettings.users[0], 'downloading');
    expect(mockManager.get_torrents).toHaveBeenCalledWith(null, 'downloading');
    
    const filterRow = replyOptions.components[1];
    const [allBtn, dlBtn, compBtn, pauseBtn] = filterRow.components;
    
    expect(allBtn.data.style).toBe(ButtonStyle.Secondary);
    expect(dlBtn.data.style).toBe(ButtonStyle.Primary); // Active
    expect(compBtn.data.style).toBe(ButtonStyle.Secondary);
    expect(pauseBtn.data.style).toBe(ButtonStyle.Secondary);
  });

  test('handleInteraction routes dc_status correctly', async () => {
    const mockInteraction: any = {
      isStringSelectMenu: () => false,
      isButton: () => true,
      customId: 'dc_status:completed',
      update: mock(async () => ({}))
    };

    await handleInteraction(mockInteraction, mockManager, mockSettings.users[0]);
    
    expect(mockManager.get_torrents).toHaveBeenCalledWith(null, 'completed');
    expect(mockInteraction.update).toHaveBeenCalled();
  });
});
