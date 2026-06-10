import { describe, expect, test } from 'bun:test';
import { RedisEmulator } from '../src/redis_helper';

describe('RedisEmulator', () => {
  test('set, get and exists', async () => {
    const redis = new RedisEmulator();
    
    expect(await redis.exists('mykey')).toBe(false);
    expect(await redis.get('mykey')).toBeNull();

    await redis.set('mykey', 'value1');
    expect(await redis.exists('mykey')).toBe(true);
    expect(await redis.get('mykey')).toBe('value1');
  });

  test('delete removes keys', async () => {
    const redis = new RedisEmulator();
    await redis.set('mykey', 'val');
    expect(await redis.exists('mykey')).toBe(true);

    await redis.delete('mykey');
    expect(await redis.exists('mykey')).toBe(false);
    expect(await redis.get('mykey')).toBeNull();
  });

  test('keys expire after timeout', async () => {
    const redis = new RedisEmulator();
    // Set key to expire in 1 second
    await redis.set('mykey', 'val', 1);
    expect(await redis.exists('mykey')).toBe(true);

    // Wait 1.2 seconds
    await new Promise(resolve => setTimeout(resolve, 1200));

    expect(await redis.exists('mykey')).toBe(false);
    expect(await redis.get('mykey')).toBeNull();
  });

  test('overwriting key clears old expiration', async () => {
    const redis = new RedisEmulator();
    // Expiration scheduled for 1 second
    await redis.set('mykey', 'val1', 1);
    
    // Overwrite without expiration
    await redis.set('mykey', 'val2');

    // Wait 1.2 seconds
    await new Promise(resolve => setTimeout(resolve, 1200));

    // Key should still exist because overwrite cleared the timeout!
    expect(await redis.exists('mykey')).toBe(true);
    expect(await redis.get('mykey')).toBe('val2');
  });
});
