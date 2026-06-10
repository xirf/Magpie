import { readdirSync, readFileSync, writeFileSync, mkdirSync, existsSync } from 'fs';
import { join } from 'path';

function parsePo(filePath: string): Record<string, string> {
  const content = readFileSync(filePath, 'utf-8');
  const lines = content.split(/\r?\n/);
  const result: Record<string, string> = {};

  let currentMsgid = '';
  let currentMsgstr = '';
  let mode: 'id' | 'str' | null = null;

  const saveEntry = () => {
    if (currentMsgid) {
      const key = currentMsgid;
      const val = currentMsgstr || currentMsgid; // fallback to key if empty
      result[key] = val;
    }
    currentMsgid = '';
    currentMsgstr = '';
    mode = null;
  };

  for (let line of lines) {
    line = line.trim();
    if (line.startsWith('#')) {
      continue; // skip comments
    }

    if (line.startsWith('msgid ')) {
      saveEntry();
      mode = 'id';
      const contentStr = line.slice(6).trim();
      if (contentStr.startsWith('"') && contentStr.endsWith('"')) {
        currentMsgid += JSON.parse(contentStr);
      }
    } else if (line.startsWith('msgstr ')) {
      mode = 'str';
      const contentStr = line.slice(7).trim();
      if (contentStr.startsWith('"') && contentStr.endsWith('"')) {
        currentMsgstr += JSON.parse(contentStr);
      }
    } else if (line.startsWith('"') && line.endsWith('"')) {
      const parsed = JSON.parse(line);
      if (mode === 'id') {
        currentMsgid += parsed;
      } else if (mode === 'str') {
        currentMsgstr += parsed;
      }
    }
  }
  saveEntry(); // save last entry

  // remove metadata key
  delete result[''];

  return result;
}

const localesDir = join(import.meta.dir, '../source/src/locales');
const outputDir = join(import.meta.dir, '../src/locales');

if (!existsSync(outputDir)) {
  mkdirSync(outputDir, { recursive: true });
}

if (existsSync(localesDir)) {
  const langs = readdirSync(localesDir);
  for (const lang of langs) {
    const poPath = join(localesDir, lang, 'LC_MESSAGES/messages.po');
    if (existsSync(poPath)) {
      console.log(`Parsing PO file for language: ${lang}`);
      const translations = parsePo(poPath);
      const outputPath = join(outputDir, `${lang}.json`);
      writeFileSync(outputPath, JSON.stringify(translations, null, 2), 'utf-8');
      console.log(`Saved translations to ${outputPath}`);
    }
  }
} else {
  console.error(`Locales directory not found at: ${localesDir}`);
}
