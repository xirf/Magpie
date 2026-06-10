import { createClient } from 'redis';

export class RedisEmulator {
  private storage = new Map<string, string>();
  private timeouts = new Map<string, Timer>();

  async get(key: string): Promise<string | null> {
    return this.storage.get(key) ?? null;
  }

  async set(key: string, value: string, ex?: number): Promise<void> {
    this.storage.set(key, value);

    const existing = this.timeouts.get(key);
    if (existing) {
      clearTimeout(existing);
      this.timeouts.delete(key);
    }

    if (ex) {
      const timeout = setTimeout(() => {
        this.storage.delete(key);
        this.timeouts.delete(key);
      }, ex * 1000);
      timeout.unref?.();
      this.timeouts.set(key, timeout);
    }
  }

  async delete(key: string): Promise<void> {
    this.storage.delete(key);
    const existing = this.timeouts.get(key);
    if (existing) {
      clearTimeout(existing);
      this.timeouts.delete(key);
    }
  }

  async exists(key: string): Promise<boolean> {
    return this.storage.has(key);
  }
}

export class RedisWrapper {
  private url: string | null;
  private client: any;
  private isEmulator = false;

  constructor(url?: string | null) {
    this.url = url || null;
  }

  async connect(): Promise<void> {
    if (!this.url) {
      console.warn("Redis URL not configured. Using in-memory storage");
      this.client = new RedisEmulator();
      this.isEmulator = true;
      return;
    }

    try {
      const redisClient = createClient({ url: this.url });
      redisClient.on('error', (err) => {
        console.error("Redis Client Error", err);
      });
      await redisClient.connect();
      await redisClient.ping();
      this.client = redisClient;
      this.isEmulator = false;
      console.log("Connected to Redis successfully");
    } catch (e) {
      console.warn(`Redis connection failed (${e}), falling back to in-memory storage`);
      this.client = new RedisEmulator();
      this.isEmulator = true;
    }
  }

  async get(key: string): Promise<string | null> {
    const val = await this.client.get(key);
    return val ?? null;
  }

  async set(key: string, value: any, ex?: number): Promise<void> {
    const stringVal = value === null || value === undefined ? '' : String(value);
    if (this.isEmulator) {
      await this.client.set(key, stringVal, ex);
    } else {
      if (ex) {
        await this.client.set(key, stringVal, { EX: ex });
      } else {
        await this.client.set(key, stringVal);
      }
    }
  }

  async delete(key: string): Promise<void> {
    if (this.isEmulator) {
      await this.client.delete(key);
    } else {
      await this.client.del(key);
    }
  }

  async exists(key: string): Promise<boolean> {
    if (this.isEmulator) {
      return await this.client.exists(key);
    } else {
      const count = await this.client.exists(key);
      return count > 0;
    }
  }
}
