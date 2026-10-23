use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    /// Code sent to the front end and used as the key in preset `labels`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Zh,
    }
}

fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// `app.language` is `auto` (follow the system), `zh-CN` or `en`.
pub fn resolve(setting: &str, system_locale: Option<&str>) -> Lang {
    let tag = match setting {
        "" | "auto" => system_locale.unwrap_or("en"),
        explicit => explicit,
    };
    if tag.to_ascii_lowercase().starts_with("zh") {
        Lang::Zh
    } else {
        Lang::En
    }
}

pub fn apply(cfg: &AppConfig) -> Lang {
    let lang = resolve(&cfg.app.language, sys_locale::get_locale().as_deref());
    set(lang);
    lang
}

/// Picks the translation for the current language, falling back to the default label.
pub fn localized(label: &str, labels: &BTreeMap<String, String>) -> String {
    localized_for(current(), label, labels)
}

pub fn localized_for(lang: Lang, label: &str, labels: &BTreeMap<String, String>) -> String {
    labels
        .get(lang.code())
        .cloned()
        .unwrap_or_else(|| label.to_string())
}

/// `t!("中文 {x}", "English {x}")`: formats the string for the current language.
macro_rules! t {
    ($zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $crate::i18n::current() {
            $crate::i18n::Lang::Zh => format!($zh $(, $arg)*),
            $crate::i18n::Lang::En => format!($en $(, $arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_setting_resolves_against_the_system_locale() {
        assert_eq!(resolve("zh-CN", Some("en-US")), Lang::Zh);
        assert_eq!(resolve("en", Some("zh-CN")), Lang::En);
        assert_eq!(resolve("auto", Some("zh-Hans-CN")), Lang::Zh);
        assert_eq!(resolve("auto", Some("ZH-TW")), Lang::Zh);
        assert_eq!(resolve("auto", Some("fr-FR")), Lang::En);
        assert_eq!(resolve("", None), Lang::En);
    }
}
