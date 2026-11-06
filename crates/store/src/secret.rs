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

/// `s` with anything secret made safe (card #96), for text the app writes from a subagent's
/// report: a sign-in link (a URL with `signin`, `sign-in`, `login` or `log-in`) becomes "[sign-in link hidden]", a
/// `Bearer` header's value becomes "[hidden]", a token or key the board refuses becomes
/// "[hidden]" with the rest of it, and the bare word "password" becomes "pass word". So a
/// finding such as "a password in config.rs" is still recorded, and what it quotes never is.
pub fn mask_secrets(s: &str) -> String {
    static LINK: OnceLock<Regex> = OnceLock::new();
    static BEARER: OnceLock<Regex> = OnceLock::new();
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    static WORD: OnceLock<Regex> = OnceLock::new();
    let link = LINK.get_or_init(|| Regex::new(r#"(?i)\S*(sign-?in|log-?in)[^\s"'<>),\]]*"#).expect("link pattern"));
    let bearer = BEARER.get_or_init(|| Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=-]+").expect("bearer pattern"));
    let token = TOKEN.get_or_init(|| Regex::new(r"(?i)(ct=|customtoken|idtoken|eyJ[A-Za-z0-9_-]{6,}|sk_(live|test))[A-Za-z0-9_.%=-]*").expect("mask pattern"));
    let word = WORD.get_or_init(|| Regex::new(r"(?i)(pass)(word)").expect("word pattern"));
    let out = link.replace_all(s, "[sign-in link hidden]");
    let out = bearer.replace_all(&out, "$1 [hidden]");
    let out = token.replace_all(&out, "[hidden]");
    word.replace_all(&out, "$1 $2").into_owned()
}

/// `s` with each token or key, with the rest of it, as "[hidden]", and nothing else changed: for
/// text that is shown but never stored, such as a card's step progress (card #106). Unlike
/// `mask_secrets`, "ct=" counts only at a word's start, so "--select=fast" stays as it is.
pub fn mask_tokens(s: &str) -> String {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    let token = TOKEN.get_or_init(|| Regex::new(r"(?i)(\bct=|customtoken|idtoken|eyJ[A-Za-z0-9_-]{6,}|sk_(live|test))[A-Za-z0-9_.%=-]*").expect("mask pattern"));
    token.replace_all(s, "[hidden]").into_owned()
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
