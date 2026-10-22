//! What a language is called in itself, and the keyboard layout that types it.

/// Languages people are likely to switch between: the code, its own name, its layout (XKB's
/// name for it).
const KNOWN: &[(&str, &str, &str)] = &[
    ("ar", "العربية", "ara"),
    ("bg", "Български", "bg"),
    ("cs", "Čeština", "cz"),
    ("da", "Dansk", "dk"),
    ("de", "Deutsch", "de"),
    ("el", "Ελληνικά", "gr"),
    ("en", "English", "us"),
    ("es", "Español", "es"),
    ("fa", "فارسی", "ir"),
    ("fi", "Suomi", "fi"),
    ("fr", "Français", "fr"),
    ("he", "עברית", "il"),
    ("hi", "हिन्दी", "in"),
    ("hu", "Magyar", "hu"),
    ("it", "Italiano", "it"),
    ("ja", "日本語", "jp"),
    ("ka", "ქართული", "ge"),
    ("ko", "한국어", "kr"),
    ("nb", "Norsk", "no"),
    ("nl", "Nederlands", "nl"),
    ("pl", "Polski", "pl"),
    ("pt", "Português", "pt"),
    ("ro", "Română", "ro"),
    ("ru", "Русский", "ru"),
    ("sv", "Svenska", "se"),
    ("tr", "Türkçe", "tr"),
    ("uk", "Українська", "ua"),
    ("vi", "Tiếng Việt", "vn"),
    ("zh", "中文", "cn"),
];

/// The two-letter code at the start of a language or locale name: "tr_TR.UTF-8" → "tr".
pub fn code(id: &str) -> String {
    id.split(['_', '-', '.', '@'])
        .next()
        .unwrap_or(id)
        .to_lowercase()
}

/// The language's own name, or its code in capitals when it isn't known here.
pub fn name(id: &str) -> String {
    let code = code(id);
    KNOWN
        .iter()
        .find(|(c, _, _)| *c == code)
        .map_or_else(|| code.to_uppercase(), |(_, name, _)| (*name).to_owned())
}

/// The layout that types the language.
pub fn layout_for(code: &str) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, _, layout)| *layout)
}

/// The entries of a list as macOS's `defaults read` prints it: `("en-US", tr)`, quoted or not.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn defaults_list(text: &str) -> Vec<String> {
    text.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// A `Language` for a system's name of it.
pub fn language(id: &str) -> super::Language {
    super::Language {
        id: id.to_owned(),
        short: code(id).to_uppercase(),
        name: name(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_are_named_in_themselves() {
        assert_eq!(code("tr_TR.UTF-8"), "tr");
        assert_eq!(name("en-US"), "English");
        assert_eq!(name("xx_YY"), "XX");
        assert_eq!(layout_for("ja"), Some("jp"));
    }

    #[test]
    fn macos_lists_its_languages_quoted_or_not() {
        let said = "(\n    \"en-US\",\n    tr,\n    \"zh-Hant-TW\"\n)";
        assert_eq!(defaults_list(said), ["en-US", "tr", "zh-Hant-TW"]);
        assert!(defaults_list("(\n)").is_empty());
    }
}
