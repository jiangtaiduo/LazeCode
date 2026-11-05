use lazecode_core::Language;

/// Select an English or Chinese string for the active language.
pub fn t<'a>(lang: Language, en: &'a str, zh: &'a str) -> &'a str {
    if lang.is_zh() {
        zh
    } else {
        en
    }
}

/// `tr!(model, "English", "中文")` using the model's language.
#[macro_export]
macro_rules! tr {
    ($model:expr, $en:expr, $zh:expr) => {
        $crate::i18n::t($model.settings.language, $en, $zh)
    };
}
