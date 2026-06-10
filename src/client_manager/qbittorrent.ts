import { Settings } from '../settings';

export interface Torrent {
  hash: string;
  name: string;
  progress: number;
  dlspeed: number;
  state: string;
  size: number;
  eta: number;
  category: string | null;
  save_path?: string | null;
  content_path?: string | null;
}

export class QBittorrentManager {
  private host: string;
  private user: string;
  private pass: string;
  private sid: string | null = null;
  private isV5: boolean | null = null;

  constructor(settings: Settings) {
    this.host = settings.client.host.replace(/\/+$/, '');
    this.user = settings.client.user;
    this.pass = settings.client.password;
  }

  private async getIsV5(): Promise<boolean> {
    if (this.isV5 === null) {
      try {
        if (!this.sid) {
          await this.login();
        }
        const res = await fetch(`${this.host}/api/v2/app/version`, {
          headers: {
            'Cookie': this.sid || '',
            'Referer': this.host,
            'Origin': this.host,
          }
        });
        if (res.ok) {
          const version = await res.text();
          this.isV5 = version.trim().toLowerCase().startsWith('v5') || /^[5-9]\./.test(version.trim());
        } else {
          this.isV5 = false;
        }
      } catch {
        this.isV5 = false;
      }
    }
    return this.isV5;
  }

  private async login(): Promise<void> {
    const params = new URLSearchParams();
    params.append('username', this.user);
    params.append('password', this.pass);

    const res = await fetch(`${this.host}/api/v2/auth/login`, {
      method: 'POST',
      body: params,
      headers: {
        'Referer': this.host,
        'Origin': this.host,
      }
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(`qBittorrent login failed (${res.status}): ${text}`);
    }

    const setCookie = res.headers.get('set-cookie');
    const match = setCookie?.match(/(QBT_SID_\d+|SID)=([^;]+)/i);
    if (!match) {
      throw new Error('qBittorrent login did not return SID cookie');
    }
    this.sid = `${match[1]}=${match[2]}`;
  }

  private async request(path: string, options: RequestInit = {}): Promise<Response> {
    if (!this.sid) {
      await this.login();
    }

    const headers = {
      ...(options.headers || {}),
      'Cookie': this.sid || '',
      'Referer': this.host,
      'Origin': this.host,
    };

    let res = await fetch(`${this.host}${path}`, {
      ...options,
      headers,
    });

    if (res.status === 403) {
      // Session expired, relogin and retry
      await this.login();
      const retryHeaders = {
        ...(options.headers || {}),
        'Cookie': this.sid || '',
        'Referer': this.host,
        'Origin': this.host,
      };
      res = await fetch(`${this.host}${path}`, {
        ...options,
        headers: retryHeaders,
      });
    }

    if (!res.ok) {
      throw new Error(`qBittorrent API request failed (${res.status}): ${res.statusText}`);
    }

    return res;
  }

  async add_magnet(magnet_link: string | string[], category?: string | null): Promise<boolean> {
    const cleanCategory = category === 'None' ? null : category;
    const body = new URLSearchParams();
    body.append('urls', Array.isArray(magnet_link) ? magnet_link.join('\n') : magnet_link);
    if (cleanCategory) {
      body.append('category', cleanCategory);
    }

    const res = await this.request('/api/v2/torrents/add', {
      method: 'POST',
      body,
    });
    return res.ok;
  }

  async add_torrent(file_path: string, category?: string | null): Promise<boolean> {
    const cleanCategory = category === 'None' ? null : category;
    const form = new FormData();
    const file = Bun.file(file_path);
    form.append('torrents', file, file.name || 'torrent.torrent');
    if (cleanCategory) {
      form.append('category', cleanCategory);
    }

    const res = await this.request('/api/v2/torrents/add', {
      method: 'POST',
      body: form,
    });
    return res.ok;
  }

  async resume_all(): Promise<void> {
    const isV5 = await this.getIsV5();
    const endpoint = isV5 ? '/api/v2/torrents/start' : '/api/v2/torrents/resume';
    const params = new URLSearchParams();
    params.append('hashes', 'all');
    await this.request(endpoint, {
      method: 'POST',
      body: params,
    });
  }

  async pause_all(): Promise<void> {
    const isV5 = await this.getIsV5();
    const endpoint = isV5 ? '/api/v2/torrents/stop' : '/api/v2/torrents/pause';
    const params = new URLSearchParams();
    params.append('hashes', 'all');
    await this.request(endpoint, {
      method: 'POST',
      body: params,
    });
  }

  async resume(torrent_hash: string): Promise<void> {
    const isV5 = await this.getIsV5();
    const endpoint = isV5 ? '/api/v2/torrents/start' : '/api/v2/torrents/resume';
    const params = new URLSearchParams();
    params.append('hashes', torrent_hash);
    await this.request(endpoint, {
      method: 'POST',
      body: params,
    });
  }

  async pause(torrent_hash: string): Promise<void> {
    const isV5 = await this.getIsV5();
    const endpoint = isV5 ? '/api/v2/torrents/stop' : '/api/v2/torrents/pause';
    const params = new URLSearchParams();
    params.append('hashes', torrent_hash);
    await this.request(endpoint, {
      method: 'POST',
      body: params,
    });
  }

  async delete_one_no_data(torrent_hash: string): Promise<void> {
    const params = new URLSearchParams();
    params.append('hashes', torrent_hash);
    params.append('deleteFiles', 'false');
    await this.request('/api/v2/torrents/delete', {
      method: 'POST',
      body: params,
    });
  }

  async delete_one_data(torrent_hash: string): Promise<void> {
    const params = new URLSearchParams();
    params.append('hashes', torrent_hash);
    params.append('deleteFiles', 'true');
    await this.request('/api/v2/torrents/delete', {
      method: 'POST',
      body: params,
    });
  }

  async delete_all_no_data(): Promise<void> {
    const torrents = await this.get_torrents();
    if (torrents.length === 0) return;
    const hashes = torrents.map(t => t.hash).join('|');

    const params = new URLSearchParams();
    params.append('hashes', hashes);
    params.append('deleteFiles', 'false');
    await this.request('/api/v2/torrents/delete', {
      method: 'POST',
      body: params,
    });
  }

  async delete_all_data(): Promise<void> {
    const torrents = await this.get_torrents();
    if (torrents.length === 0) return;
    const hashes = torrents.map(t => t.hash).join('|');

    const params = new URLSearchParams();
    params.append('hashes', hashes);
    params.append('deleteFiles', 'true');
    await this.request('/api/v2/torrents/delete', {
      method: 'POST',
      body: params,
    });
  }

  async get_categories(): Promise<string[] | undefined> {
    const res = await this.request('/api/v2/torrents/categories');
    const categories = (await res.json()) as Record<string, any>;
    const keys = Object.keys(categories);
    return keys.length > 0 ? keys : undefined;
  }

  async set_torrents_category(category: string, torrent_hashes: string | string[]): Promise<void> {
    const hashes = Array.isArray(torrent_hashes) ? torrent_hashes.join('|') : torrent_hashes;
    const params = new URLSearchParams();
    params.append('hashes', hashes);
    params.append('category', category);
    await this.request('/api/v2/torrents/setCategory', {
      method: 'POST',
      body: params,
    });
  }

  async get_torrent(torrent_hash: string, status_filter?: string | null): Promise<Torrent | null> {
    const torrents = await this.get_torrents(torrent_hash, status_filter);
    return torrents.length > 0 ? torrents[0] : null;
  }

  async get_torrents(torrent_hash?: string | null, status_filter?: string | null): Promise<Torrent[]> {
    let query = '';
    const params = new URLSearchParams();
    if (torrent_hash) {
      params.append('hashes', torrent_hash);
    }
    if (status_filter) {
      let mappedFilter = status_filter;
      const isV5 = await this.getIsV5();
      if (isV5) {
        if (status_filter === 'paused') {
          mappedFilter = 'stopped';
        } else if (status_filter === 'resumed') {
          mappedFilter = 'running';
        }
      }
      params.append('filter', mappedFilter);
    }
    if (params.toString()) {
      query = `?${params.toString()}`;
    }

    const res = await this.request(`/api/v2/torrents/info${query}`);
    const data = (await res.json()) as any[];

    return data.map(t => ({
      hash: t.hash,
      name: t.name,
      progress: t.progress,
      dlspeed: t.dlspeed,
      state: t.state,
      size: t.size,
      eta: t.eta,
      category: t.category || null,
      save_path: t.save_path || null,
      content_path: t.content_path || null,
    }));
  }

  async edit_category(name: string, save_path: string): Promise<void> {
    const params = new URLSearchParams();
    params.append('category', name);
    params.append('savePath', save_path);
    await this.request('/api/v2/torrents/editCategory', {
      method: 'POST',
      body: params,
    });
  }

  async create_category(name: string, save_path: string): Promise<void> {
    const params = new URLSearchParams();
    params.append('category', name);
    params.append('savePath', save_path);
    await this.request('/api/v2/torrents/createCategory', {
      method: 'POST',
      body: params,
    });
  }

  async remove_category(name: string): Promise<void> {
    const params = new URLSearchParams();
    params.append('categories', name);
    await this.request('/api/v2/torrents/removeCategories', {
      method: 'POST',
      body: params,
    });
  }

  async check_connection(): Promise<string> {
    const res = await this.request('/api/v2/app/version');
    return await res.text();
  }

  async export_torrent(torrent_hash: string): Promise<{ buffer: Buffer; name: string }> {
    const torrentInfo = await this.get_torrent(torrent_hash);
    const torrentName = torrentInfo ? torrentInfo.name : 'torrent';

    const res = await this.request(`/api/v2/torrents/export?hash=${torrent_hash}`);
    const arrayBuffer = await res.arrayBuffer();

    return {
      buffer: Buffer.from(arrayBuffer),
      name: `${torrentName}.torrent`,
    };
  }

  async get_speed_limit_mode(): Promise<boolean> {
    const res = await this.request('/api/v2/transfer/speedLimitsMode');
    const text = await res.text();
    return text === '1';
  }

  async toggle_speed_limit(): Promise<boolean> {
    await this.request('/api/v2/transfer/toggleSpeedLimitsMode', {
      method: 'POST',
    });
    // Tiny delay for qBittorrent to complete state change internally
    await new Promise(resolve => setTimeout(resolve, 200));
    return await this.get_speed_limit_mode();
  }
}
