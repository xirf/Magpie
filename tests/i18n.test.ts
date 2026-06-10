import { describe, expect, test } from 'bun:test';
import { t, availableLocales } from '../src/i18n';

describe('I18n Translator', () => {
  test('availableLocales includes correct items', () => {
    expect(availableLocales).toContain('en');
    expect(availableLocales).toContain('it');
    expect(availableLocales).toContain('es');
    expect(availableLocales).toContain('pt');
    expect(availableLocales).toContain('ru');
    expect(availableLocales).toContain('uk');
  });

  test('t returns correct simple translation string', () => {
    // English
    expect(t("You are not authorized to use this bot", "en")).toBe("You are not authorized to use this bot");
    // Italian
    expect(t("You are not authorized to use this bot", "it")).toBe("Non sei autorizzato ad usare questo bot");
    // Spanish
    expect(t("You are not authorized to use this bot", "es")).toBe("No estás autorizado para usar este bot");
  });

  test('t does key fallback for unknown keys', () => {
    expect(t("This key does not exist anywhere", "en")).toBe("This key does not exist anywhere");
    expect(t("This key does not exist anywhere", "it")).toBe("This key does not exist anywhere");
  });

  test('t substitutes format variables', () => {
    // English
    expect(
      t("Torrent {name} has finished downloading!", "en", { name: "ubuntu-24.04-desktop-amd64.iso" })
    ).toBe("Torrent ubuntu-24.04-desktop-amd64.iso has finished downloading!");

    // Italian
    expect(
      t("Torrent {name} has finished downloading!", "it", { name: "ubuntu-24.04-desktop-amd64.iso" })
    ).toBe("Il torrent ubuntu-24.04-desktop-amd64.iso è stato scaricato!");

    // Italian with multiple variables (e.g., Stats)
    const statsText = t(
      "**============SYSTEM============**\n**CPU Usage:** {cpu_usage}%\n" +
      "**CPU Temp:** {cpu_temp}°C\n**Free Memory:** {free_memory} of {total_memory} ({memory_percent}%)\n" +
      "**Disks usage:** {disk_used} of {disk_total} ({disk_percent}%)",
      "it",
      {
        cpu_usage: 12,
        cpu_temp: 45,
        free_memory: "8.5 GB",
        total_memory: "16 GB",
        memory_percent: 46,
        disk_used: "450 GB",
        disk_total: "1 TB",
        disk_percent: 45
      }
    );

    expect(statsText).toContain("SISTEMA");
    expect(statsText).toContain("**Utilizzo CPU:** 12%");
    expect(statsText).toContain("**Temperatura CPU:** 45°C");
    expect(statsText).toContain("**Memoria libera:** 8.5 GB");
    expect(statsText).toContain("**Utilizzo disco:** 450 GB");
  });
});
