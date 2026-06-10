import { describe, expect, test, beforeEach, afterEach } from 'bun:test';

// Set environment variable for test isolation to use in-memory database
process.env.DATABASE_PATH = ':memory:';

import {
  getDatabase,
  closeDatabase,
  isNotificationSent,
  markNotificationSent,
  getCachedPresignedUrl,
  cachePresignedUrl,
  syncUsers,
  saveUserToDB,
  loadUsersFromDB
} from '../src/utils/db';

describe('SQLite Database Tests', () => {
  beforeEach(() => {
    // Reset database state for test isolation
    closeDatabase();
    getDatabase();
  });

  afterEach(() => {
    closeDatabase();
  });

  test('Sent Notification tracking persists and works', () => {
    expect(isNotificationSent('hash-1')).toBe(false);
    
    markNotificationSent('hash-1');
    expect(isNotificationSent('hash-1')).toBe(true);
    expect(isNotificationSent('hash-2')).toBe(false);
  });

  test('Presigned URL cache respects 75% remaining policy', () => {
    const key = 'folder/video.mp4';
    const originalNow = Date.now;
    
    let fakeTime = 100000; // custom timestamp in seconds
    Date.now = () => fakeTime * 1000;

    try {
      // Expiry is 100 seconds
      cachePresignedUrl(key, 'http://test-url.com/video.mp4', 100);

      // Scenario 1: 10 seconds elapsed (90% remaining). Should reuse.
      fakeTime = 100010;
      expect(getCachedPresignedUrl(key)).toBe('http://test-url.com/video.mp4');

      // Scenario 2: 24 seconds elapsed (76% remaining). Should reuse.
      fakeTime = 100024;
      expect(getCachedPresignedUrl(key)).toBe('http://test-url.com/video.mp4');

      // Scenario 3: 26 seconds elapsed (74% remaining). Should NOT reuse.
      fakeTime = 100026;
      expect(getCachedPresignedUrl(key)).toBeNull();
    } finally {
      Date.now = originalNow;
    }
  });

  test('Authorized users sync and save properly', () => {
    const yamlUsers = [
      {
        user_id: 111,
        discord_id: 'discord-111',
        role: 'administrator',
        locale: 'es',
        notify: true,
        notification_filter: []
      },
      {
        user_id: 222,
        discord_id: null,
        role: 'manager',
        locale: 'en',
        notify: false,
        notification_filter: ['movies']
      }
    ];

    const synced = syncUsers(yamlUsers);
    expect(synced.length).toBe(2);
    
    const u1 = synced.find(u => u.user_id === 111);
    expect(u1).toBeDefined();
    expect(u1?.role).toBe('administrator');
    expect(u1?.locale).toBe('es');
    expect(u1?.notify).toBe(true);

    const u2 = synced.find(u => u.user_id === 222);
    expect(u2).toBeDefined();
    expect(u2?.discord_id).toBeNull();
    expect(u2?.role).toBe('manager');
    expect(u2?.notify).toBe(false);
    expect(u2?.notification_filter).toEqual(['movies']);

    // Dynamic update test
    u2!.role = 'administrator';
    saveUserToDB(u2!);

    const updatedUsers = loadUsersFromDB();
    const updatedU2 = updatedUsers.find(u => u.user_id === 222);
    expect(updatedU2?.role).toBe('administrator');
  });
});
