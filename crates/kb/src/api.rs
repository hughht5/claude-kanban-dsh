//! Calling the running app's local API. `kb` never writes the board itself: the app is
//! the only writer, so these commands need it running.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

fn api_file() -> PathBuf {
    crate::data_dir().join("api.json")
}

fn post(path_tail: impl Fn(&str) -> String, body: &Value, timeout: Duration) -> Result<Value, String> {
    let text = std::fs::read_to_string(api_file()).map_err(|_| format!("the {} app isn't running (no api.json in {})", crate::app_label(), crate::data_dir().display()))?;
    let info: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let port = info["port"].as_u64().ok_or("api.json has no port")?;
    let token = info["token"].as_str().ok_or("api.json has no token")?;
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], port as u16).into(), Duration::from_millis(800))
        .map_err(|_| format!("the {} app isn't running", crate::app_label()))?;
    s.set_read_timeout(Some(timeout)).ok();
    let payload = body.to_string();
    let session = std::env::var("KANBAN_SESSION").map(|v| format!("X-Kanban-Session: {v}\r\n")).unwrap_or_default();
    write!(
        s,
        "POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{session}Connection: close\r\n\r\n{payload}",
        path_tail(token),
        payload.len()
    )
    .map_err(|e| e.to_string())?;
    let mut resp = String::new();
    s.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    let body = resp.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    if resp.starts_with("HTTP/1.1 403") {
        return Err("the app refused the token; is api.json from an older run?".into());
    }
    Ok(serde_json::from_str(body).unwrap_or(Value::Null))
}

/// Runs a kanban_* tool in the app and returns its result.
pub fn tool(name: &str, args: Value) -> Result<Value, String> {
    let r = post(|t| format!("/tool/{t}/{name}"), &args, Duration::from_secs(10))?;
    if r["ok"].as_bool() == Some(true) {
        Ok(r["result"].clone())
    } else {
        Err(r["error"].as_str().unwrap_or("the app gave no answer").to_string())
    }
}

/// `kb status`: Claude Code's statusLine. Sends the JSON on to the app and prints one line.
pub fn status() -> String {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let v: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    if let Ok(session) = std::env::var("KANBAN_SESSION") {
        let _ = post(|t| format!("/status/{t}/{session}"), &v, Duration::from_millis(1500));
    }
    let card = std::env::var("KANBAN_CARD").map(|n| format!("#{n}")).unwrap_or_else(|_| "kanban".into());
    let model = v.pointer("/model/display_name").and_then(Value::as_str).unwrap_or("");
    let week = v.pointer("/rate_limits/seven_day/used_percentage").and_then(Value::as_f64);
    let ctx = v.pointer("/context_window/used_percentage").and_then(Value::as_f64);
    let mut parts = vec![card];
    if !model.is_empty() {
        parts.push(model.to_string());
    }
    if let Some(c) = ctx {
        parts.push(format!("context {c:.0}%"));
    }
    if let Some(w) = week {
        parts.push(format!("7-day {w:.0}%"));
    }
    parts.join(" · ")
}

/// `--name value` pairs into a JSON object; a value that parses as JSON is kept as JSON.
pub fn fields(args: &[String]) -> Result<Value, String> {
    let mut out = serde_json::Map::new();
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let name = k.strip_prefix("--").ok_or_else(|| format!("expected --field, got {k}"))?;
        let v = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
        // Objects, lists, true/false and null are JSON; everything else is text.
        let value = serde_json::from_str::<Value>(v)
            .ok()
            .filter(|j| j.is_object() || j.is_array() || j.is_boolean() || j.is_null())
            .unwrap_or_else(|| json!(v));
        out.insert(name.to_string(), value);
    }
    Ok(Value::Object(out))
}

/// `kb mcp`: an MCP server over stdio that forwards each JSON-RPC message to the app's MCP route.
/// Outside a card session it is the external route, with the session-control tools; inside one
/// (KANBAN_SESSION set) it is that session's own route, so a card session never gets them.
/// It reads api.json for every message, so it keeps working after the app restarts.
pub fn mcp_bridge() {
    use std::io::BufRead;
    let route = std::env::var("KANBAN_SESSION").unwrap_or_else(|_| "external".into());
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else {
            // A notification: pass it on, no reply.
            let _ = post(|t| format!("/mcp/{t}/{route}"), &msg, Duration::from_secs(10));
            continue;
        };
        let reply = match post(|t| format!("/mcp/{t}/{route}"), &msg, Duration::from_secs(120)) {
            Ok(Value::Null) => continue,
            Ok(v) => v,
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": e } }),
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}
