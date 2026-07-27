/**
 * Languages offered by the selection-toolbar translate panel.
 *
 * Codes mirror `LANGUAGE_ENGLISH_NAMES` in
 * `src-tauri/src/selection_toolbar/languages.rs` — the backend renders prompt
 * placeholders from the same codes. Labels are native names so the list reads
 * the same in every UI locale (Google Translate style).
 */
export interface SelectionTranslateLanguage {
  code: string;
  /** Native display name shown in pickers. */
  native: string;
  /** English name, used for search matching. */
  english: string;
}

export const SELECTION_TRANSLATE_LANGUAGES: readonly SelectionTranslateLanguage[] = [
  { code: 'zh-CN', native: '简体中文', english: 'Simplified Chinese' },
  { code: 'zh-TW', native: '繁體中文', english: 'Traditional Chinese' },
  { code: 'en', native: 'English', english: 'English' },
  { code: 'ja', native: '日本語', english: 'Japanese' },
  { code: 'ko', native: '한국어', english: 'Korean' },
  { code: 'fr', native: 'Français', english: 'French' },
  { code: 'de', native: 'Deutsch', english: 'German' },
  { code: 'es', native: 'Español', english: 'Spanish' },
  { code: 'ru', native: 'Русский', english: 'Russian' },
  { code: 'hi', native: 'हिन्दी', english: 'Hindi' },
  { code: 'ar', native: 'العربية', english: 'Arabic' },
  { code: 'pt', native: 'Português', english: 'Portuguese' },
  { code: 'it', native: 'Italiano', english: 'Italian' },
  { code: 'nl', native: 'Nederlands', english: 'Dutch' },
  { code: 'pl', native: 'Polski', english: 'Polish' },
  { code: 'tr', native: 'Türkçe', english: 'Turkish' },
  { code: 'th', native: 'ไทย', english: 'Thai' },
  { code: 'vi', native: 'Tiếng Việt', english: 'Vietnamese' },
  { code: 'id', native: 'Bahasa Indonesia', english: 'Indonesian' },
  { code: 'ms', native: 'Bahasa Melayu', english: 'Malay' },
  { code: 'fil', native: 'Filipino', english: 'Filipino' },
  { code: 'uk', native: 'Українська', english: 'Ukrainian' },
  { code: 'cs', native: 'Čeština', english: 'Czech' },
  { code: 'sv', native: 'Svenska', english: 'Swedish' },
  { code: 'da', native: 'Dansk', english: 'Danish' },
  { code: 'fi', native: 'Suomi', english: 'Finnish' },
  { code: 'no', native: 'Norsk', english: 'Norwegian' },
  { code: 'el', native: 'Ελληνικά', english: 'Greek' },
  { code: 'he', native: 'עברית', english: 'Hebrew' },
  { code: 'ro', native: 'Română', english: 'Romanian' },
  { code: 'hu', native: 'Magyar', english: 'Hungarian' },
  { code: 'bg', native: 'Български', english: 'Bulgarian' },
  { code: 'sk', native: 'Slovenčina', english: 'Slovak' },
  { code: 'hr', native: 'Hrvatski', english: 'Croatian' },
  { code: 'sr', native: 'Српски', english: 'Serbian' },
  { code: 'bn', native: 'বাংলা', english: 'Bengali' },
  { code: 'ta', native: 'தமிழ்', english: 'Tamil' },
  { code: 'te', native: 'తెలుగు', english: 'Telugu' },
  { code: 'ur', native: 'اردو', english: 'Urdu' },
  { code: 'fa', native: 'فارسی', english: 'Persian' },
  { code: 'kk', native: 'Қазақша', english: 'Kazakh' },
  { code: 'mn', native: 'Монгол', english: 'Mongolian' },
  { code: 'km', native: 'ខ្មែរ', english: 'Khmer' },
  { code: 'lo', native: 'ລາວ', english: 'Lao' },
  { code: 'my', native: 'မြန်မာ', english: 'Burmese' },
];

/**
 * Map an arbitrary language code (e.g. the app locale "en-US") onto a code
 * present in {@link SELECTION_TRANSLATE_LANGUAGES} so pickers always show a
 * valid selection.
 */
export function normalizeTranslateLanguage(code: string | null | undefined): string {
  if (!code) return 'en';
  const trimmed = code.trim();
  const exact = SELECTION_TRANSLATE_LANGUAGES.find(
    (language) => language.code.toLowerCase() === trimmed.toLowerCase(),
  );
  if (exact) return exact.code;
  const base = trimmed.split('-')[0].toLowerCase();
  const baseMatch = SELECTION_TRANSLATE_LANGUAGES.find(
    (language) => language.code.toLowerCase() === base
      || language.code.toLowerCase().split('-')[0] === base,
  );
  return baseMatch?.code ?? 'en';
}

export function translateLanguageNative(code: string): string {
  return (
    SELECTION_TRANSLATE_LANGUAGES.find((language) => language.code === code)?.native ?? code
  );
}
