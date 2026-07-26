import { describe, expect, it } from 'vitest';
import ar from '../locales/ar.json';
import de from '../locales/de.json';
import enUS from '../locales/en-US.json';
import es from '../locales/es.json';
import fr from '../locales/fr.json';
import hi from '../locales/hi.json';
import ja from '../locales/ja.json';
import ko from '../locales/ko.json';
import ru from '../locales/ru.json';
import zhCN from '../locales/zh-CN.json';
import zhTW from '../locales/zh-TW.json';

const locales = { ar, de, 'en-US': enUS, es, fr, hi, ja, ko, ru, 'zh-CN': zhCN, 'zh-TW': zhTW };

describe('crash recovery locales', () => {
  it('keeps every crash recovery key available in all 11 locales', () => {
    const expected = Object.keys(enUS.crashRecovery).sort();
    for (const [locale, translations] of Object.entries(locales)) {
      expect(Object.keys(translations.crashRecovery).sort(), locale).toEqual(expected);
    }
  });
});
