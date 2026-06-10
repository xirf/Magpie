import { readFileSync, existsSync } from 'fs';
import { join } from 'path';

const loadedTranslations: Record<string, Record<string, string>> = {};
const availableLocales = ['en', 'es', 'it', 'pt', 'ru', 'uk'];

// Load translations from JSON files at startup (relative to src/)
const localesDir = join(import.meta.dir, './locales');
for (const locale of availableLocales) {
  const path = join(localesDir, `${locale}.json`);
  if (existsSync(path)) {
    try {
      loadedTranslations[locale] = JSON.parse(readFileSync(path, 'utf-8'));
    } catch (e) {
      console.error(`Failed to parse translations for ${locale}`, e);
    }
  }
}

/**
 * Translates a key for a given locale, substituting variables in the format string.
 * @param key The msgid/source text to translate.
 * @param locale The target language code (e.g. 'en', 'it').
 * @param vars Variables to substitute (e.g. { name: 'Ubuntu' } replaces '{name}').
 */
export function t(key: string, locale: string = 'en', vars?: Record<string, any>): string {
  // If translation isn't found in current locale, fallback to 'en', then fallback to key
  const translations = loadedTranslations[locale] || loadedTranslations['en'] || {};
  let template = translations[key] ?? key;

  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      template = template.replace(new RegExp(`\\{${k}\\}`, 'g'), String(v));
    }
  }

  return template;
}

export { availableLocales };
