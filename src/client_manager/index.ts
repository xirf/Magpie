import { Settings } from '../settings';
import { QBittorrentManager } from './qbittorrent';

export * from './qbittorrent';

export class ClientRepo {
  /**
   * Returns an instance of the configured ClientManager (e.g. QBittorrentManager)
   */
  static getClientManager(settings: Settings): QBittorrentManager {
    if (settings.client.type === 'qbittorrent') {
      return new QBittorrentManager(settings);
    }
    throw new Error(`Unsupported client type: ${settings.client.type}`);
  }
}
