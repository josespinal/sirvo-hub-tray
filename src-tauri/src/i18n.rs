use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

const EN: &str = include_str!("../../i18n/en.json");
const ES: &str = include_str!("../../i18n/es.json");

pub static TRANSLATIONS: Lazy<HashMap<&'static str, Value>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("en", serde_json::from_str(EN).expect("en.json"));
    m.insert("es", serde_json::from_str(ES).expect("es.json"));
    m
});

pub fn detect_locale() -> &'static str {
    let raw = sys_locale::get_locale().unwrap_or_else(|| "en".into());
    if raw.starts_with("es") { "es" } else { "en" }
}

pub fn t(lang: &str, key: &str) -> String {
    let table = TRANSLATIONS.get(lang).or_else(|| TRANSLATIONS.get("en")).expect("en");
    let mut cur = table;
    for seg in key.split('.') {
        cur = match cur.get(seg) {
            Some(v) => v,
            None => return key.to_string(),
        };
    }
    cur.as_str().unwrap_or(key).to_string()
}

/// Replace `{name}` placeholders with the matching value.
pub fn t_fmt(lang: &str, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(lang, key);
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}
