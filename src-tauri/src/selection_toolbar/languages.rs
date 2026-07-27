//! Language names for translate-prompt placeholders.
//!
//! Codes mirror `SELECTION_TRANSLATE_LANGUAGES` in the frontend
//! (`src/constants/selectionTranslateLanguages.ts`) plus the application
//! locale codes (`en-US`, `zh-CN`, …). Prompts always use English language
//! names — models follow them more reliably than native names.

const LANGUAGE_ENGLISH_NAMES: &[(&str, &str)] = &[
    ("en", "English"),
    ("en-us", "English"),
    ("zh-cn", "Simplified Chinese"),
    ("zh-tw", "Traditional Chinese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("de", "German"),
    ("es", "Spanish"),
    ("ru", "Russian"),
    ("hi", "Hindi"),
    ("ar", "Arabic"),
    ("pt", "Portuguese"),
    ("pt-br", "Brazilian Portuguese"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("tr", "Turkish"),
    ("th", "Thai"),
    ("vi", "Vietnamese"),
    ("id", "Indonesian"),
    ("ms", "Malay"),
    ("fil", "Filipino"),
    ("uk", "Ukrainian"),
    ("cs", "Czech"),
    ("sv", "Swedish"),
    ("da", "Danish"),
    ("fi", "Finnish"),
    ("no", "Norwegian"),
    ("nb", "Norwegian"),
    ("el", "Greek"),
    ("he", "Hebrew"),
    ("ro", "Romanian"),
    ("hu", "Hungarian"),
    ("bg", "Bulgarian"),
    ("sk", "Slovak"),
    ("hr", "Croatian"),
    ("sr", "Serbian"),
    ("bn", "Bengali"),
    ("ta", "Tamil"),
    ("te", "Telugu"),
    ("ur", "Urdu"),
    ("fa", "Persian"),
    ("kk", "Kazakh"),
    ("mn", "Mongolian"),
    ("km", "Khmer"),
    ("lo", "Lao"),
    ("my", "Burmese"),
];

/// English display name for a language code, falling back to the base code
/// ("en-GB" → English) and finally to the code itself so prompts stay usable
/// with codes we do not know about.
pub fn prompt_language_name(code: &str) -> String {
    let trimmed = code.trim();
    let lookup = |candidate: &str| {
        LANGUAGE_ENGLISH_NAMES
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(candidate))
            .map(|(_, name)| (*name).to_string())
    };
    if let Some(name) = lookup(trimmed) {
        return name;
    }
    if let Some((base, _)) = trimmed.split_once('-') {
        if let Some(name) = lookup(base) {
            return name;
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::prompt_language_name;

    #[test]
    fn language_names_resolve_exact_base_and_unknown_codes() {
        assert_eq!(prompt_language_name("zh-CN"), "Simplified Chinese");
        assert_eq!(prompt_language_name("en-US"), "English");
        assert_eq!(prompt_language_name("en-GB"), "English");
        assert_eq!(prompt_language_name("fr"), "French");
        assert_eq!(prompt_language_name("tlh"), "tlh");
    }
}
