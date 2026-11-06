use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// The board page's filter, unchanged, so the app refuses exactly what the page refused.
fn pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)ct=|customtoken|idtoken|eyJ[A-Za-z0-9_-]{6,}|sk_(live|test)|password")
            .expect("secret pattern")
    })
}

pub fn looks_secret(s: &str) -> bool {
    pattern().is_match(s)
}

/// The path of the first string inside `v` that looks like a secret, if any.
pub(crate) fn find_secret(key: &str, v: &Value) -> Option<String> {
    match v {
        Value::String(s) if looks_secret(s) => Some(key.to_string()),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, x)| find_secret(&format!("{key}[{i}]"), x)),
        Value::Object(map) => map
            .iter()
            .find_map(|(k, x)| find_secret(&format!("{key}.{k}"), x)),
        _ => None,
    }
}

/// The board's filter without the bare word "password" or "ct=": for prompts and templates, which
/// may tell a session never to store a password or a ct= link, but must never hold a token or key.
pub fn looks_like_token(s: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)customtoken|idtoken|eyJ[A-Za-z0-9_-]{6,}|sk_(live|test)")
            .expect("token pattern")
    })
    .is_match(s)
}
