import { describe, expect, test, beforeAll } from 'bun:test';
import { Settings } from '../src/settings';
import { QBittorrentManager } from '../src/client_manager/qbittorrent';

describe('qBittorrent Integration Test', () => {
  let settings: Settings;
  let manager: QBittorrentManager;
  let isConnected = false;

  beforeAll(async () => {
    try {
      settings = Settings.loadSettings();
      manager = new QBittorrentManager(settings);
      const version = await manager.check_connection();
      console.log(`\n[Integration] Successfully connected to qBittorrent. Version: ${version}`);
      isConnected = true;
    } catch (e) {
      console.warn(`\n[Integration] Skip: Could not connect to qBittorrent at ${settings?.client?.host} (${e})`);
      console.warn(`Please verify qBittorrent is running and WebUI is enabled under Options -> Web UI.`);
    }
  });

  test('Check connection', async () => {
    if (!isConnected) {
      console.log('Skipped connection test');
      return;
    }
    const version = await manager.check_connection();
    expect(version).toBeString();
    expect(version.length).toBeGreaterThan(0);
  });

  test('Categories CRUD operations', async () => {
    if (!isConnected) {
      console.log('Skipped categories CRUD test');
      return;
    }

    const testCat = 'bot_test_cat';
    const testPath = '';

    // Cleanup from potential previous failed runs
    try {
      await manager.remove_category(testCat);
    } catch {}

    // 1. Create category
    await manager.create_category(testCat, testPath);

    // 2. Fetch categories and verify it exists
    const categories = await manager.get_categories();
    expect(categories).toBeDefined();
    expect(categories).toContain(testCat);

    // 3. Edit category
    const newPath = '';
    await manager.edit_category(testCat, newPath);

    // 4. Remove category
    await manager.remove_category(testCat);

    // 5. Verify removed
    const categoriesAfter = await manager.get_categories();
    if (categoriesAfter) {
      expect(categoriesAfter).not.toContain(testCat);
    }
  });

  test('Speed limit toggles', async () => {
    if (!isConnected) {
      console.log('Skipped speed limit toggle test');
      return;
    }

    const initialMode = await manager.get_speed_limit_mode();
    expect(initialMode).toBeBoolean();

    // Toggle once
    const newMode = await manager.toggle_speed_limit();
    expect(newMode).toBe(!initialMode);

    // Toggle back
    const finalMode = await manager.toggle_speed_limit();
    expect(finalMode).toBe(initialMode);
  });

  test('Add, pause, delete magnet link', async () => {
    if (!isConnected) {
      console.log('Skipped magnet link lifecycle test');
      return;
    }

    // A user-provided magnet link
    const magnet = 'magnet:?xt=urn:btih:17d8ddec0f6445c935b47b799ee229f25f3a9081&dn=%E5%8B%87%E8%80%85%E6%A7%98%E3%81%AE%E5%B9%BC%E9%A6%B4%E6%9F%93%E3%81%A8%E3%81%84%E3%81%86%E8%81%B7%E6%A5%AD%E3%81%AE%E8%B2%A0%E3%81%91%E3%83%92%E3%83%AD%E3%82%A4%E3%83%B3%E3%81%AB%E8%BB%A2%E7%94%9F%E3%81%97%E3%81%9F%E3%81%AE%E3%81%A7%E3%80%81%E8%AA%BF%E5%90%88%E5%B8%AB%E3%81%AB%E3%82%B8%E3%83%A7%E3%83%96%E3%83%81%E3%82%A7%E3%83%B3%E3%82%B8%E3%81%97%E3%81%BE%E3%81%99%E3%0%82%20%E7%AC%AC01-07%E5%B7%BB%20%5BYushasama%20no%20Osananajimi%20to%20iu%20Settei%20no%20Make%20Hiroin%20ni%20Tensei%20Shita%20Node%20Chogoshi%20ni%20Jobu%20Chenji%20Shimasu%20vol%2001-07%5D&tr=http%3A%2F%2Fnyaa.tracker.wf%3A7777%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Fexodus.desync.com%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451%2Fannounce';

    // 1. Create a category to keep it separated
    const tempCat = 'bot_temp_mag_cat';
    try {
      await manager.remove_category(tempCat);
    } catch {}
    await manager.create_category(tempCat, '');

    // 2. Cleanup preexisting torrent if left from failed runs
    try {
      await manager.delete_one_data('17d8ddec0f6445c935b47b799ee229f25f3a9081');
    } catch {}

    // 3. Add magnet link
    const added = await manager.add_magnet(magnet, tempCat);
    expect(added).toBe(true);

    // Give qBittorrent 2.5 seconds to register the torrent entry
    await new Promise(resolve => setTimeout(resolve, 2500));

    // 3. Find our added torrent by category
    const torrents = await manager.get_torrents();
    const torrent = torrents.find(t => t.category === tempCat);
    expect(torrent).toBeDefined();
    const torrentHash = torrent!.hash;

    // 4. Pause the torrent
    await manager.pause(torrentHash);
    await new Promise(resolve => setTimeout(resolve, 2500));
    const pausedTorrent = await manager.get_torrent(torrentHash);
    expect(pausedTorrent).toBeDefined();
    expect(pausedTorrent!.state.toLowerCase()).toMatch(/paused|stopped/);

    // 5. Resume the torrent
    await manager.resume(torrentHash);
    await new Promise(resolve => setTimeout(resolve, 2500));
    const resumedTorrent = await manager.get_torrent(torrentHash);
    expect(resumedTorrent).toBeDefined();
    expect(resumedTorrent!.state.toLowerCase()).not.toMatch(/paused|stopped/);

    // 6. Delete the torrent
    await manager.delete_one_no_data(torrentHash);
    await new Promise(resolve => setTimeout(resolve, 2500));

    // 7. Verify it has been deleted
    const torrentsAfter = await manager.get_torrents();
    expect(torrentsAfter.find(t => t.hash === torrentHash)).toBeUndefined();

    // Clean up category
    await manager.remove_category(tempCat);
  }, 20000);
});
