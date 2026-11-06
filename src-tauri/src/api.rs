//! The app's localhost API, for its own card sessions and `kb`:
//!   POST /hook/<token>/<session>/<event>   Claude Code `http` hooks
//!   POST /status/<token>/<session>         the statusLine's JSON, sent on by `kb status`
//!   POST /mcp/<token>/<session|external>   MCP (JSON-RPC over streamable HTTP, JSON replies)
//!   POST /tool/<token>/<tool>              a kanban_* tool, for `kb`; header X-Kanban-Session names the caller
//! Bound to 127.0.0.1 only. The token is random per launch and written to the app's folder.

use crate::{dispatch, tools};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use tauri::AppHandle;

pub const PORT: u16 = 47821;

#[derive(Debug, Clone, PartialEq)]
pub struct ApiInfo {
    pub port: u16,
    pub token: String,
    pub kb_exe: Option<PathBuf>,
}

/// Binds the first free port from 47821, writes `api.json` (port and token) beside the board,
/// and serves requests on a thread each.
pub fn start(app: AppHandle, dir: PathBuf) -> Result<ApiInfo, String> {
    let token = uuid::Uuid::new_v4().simple().to_string();
    // A fixed port when it's free, else whatever the OS gives: everything reads api.json.
    let listener = (PORT..PORT + 10)
        .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok())
        .or_else(|| TcpListener::bind(("127.0.0.1", 0)).ok())
        .ok_or("no free port for the app's local API")?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let kb_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("kb.exe"))).filter(|p| p.is_file());
    std::fs::write(dir.join("api.json"), json!({ "port": port, "token": token }).to_string()).map_err(|e| e.to_string())?;
    let info = ApiInfo { port, token: token.clone(), kb_exe };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (app, token) = (app.clone(), token.clone());
            std::thread::spawn(move || {
                let _ = serve(&app, &token, stream);
            });
        }
    });
    Ok(info)
}

struct Request {
    method: String,
    path: String,
    session: Option<String>,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let (mut len, mut session) = (0usize, None);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = v.trim().parse().unwrap_or(0),
                "x-kanban-session" => session = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    let mut body = vec![0; len.min(8 * 1024 * 1024)];
    r.read_exact(&mut body)?;
    Ok(Request { method, path, session, body })
}

fn respond(mut stream: &TcpStream, status: &str, body: &Value) -> std::io::Result<()> {
    let text = if body.is_null() { String::new() } else { body.to_string() };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

fn serve(app: &AppHandle, token: &str, stream: TcpStream) -> std::io::Result<()> {
    let req = match read_request(&stream) {
        Ok(r) => r,
        Err(e) => {
            log_request(&Request { method: "?".into(), path: format!("/unreadable/{e}"), session: None, body: vec![] });
            return Err(e);
        }
    };
    log_request(&req);
    let seg: Vec<&str> = req.path.trim_start_matches('/').split('/').collect();
    if seg.len() < 2 || seg[1] != token {
        return respond(&stream, "403 Forbidden", &json!({ "error": "wrong token" }));
    }
    if req.method != "POST" {
        // Streamable HTTP clients may open a GET stream; the app has nothing to push.
        return respond(&stream, "405 Method Not Allowed", &Value::Null);
    }
    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    match (seg[0], seg.get(2), seg.get(3)) {
        ("hook", Some(session), Some(event)) => {
            dispatch::on_hook(app, session, event, &body);
            respond(&stream, "200 OK", &json!({}))
        }
        ("status", Some(session), _) => {
            dispatch::on_status(app, session, &body);
            respond(&stream, "200 OK", &json!({}))
        }
        ("mcp", Some(caller), _) => {
            let caller = (*caller != "external").then(|| caller.to_string());
            match mcp(app, caller.as_deref(), &body) {
                Some(reply) => respond(&stream, "200 OK", &reply),
                None => respond(&stream, "202 Accepted", &Value::Null),
            }
        }
        ("tool", Some(name), _) => match if crate::control::CONTROL.contains(name) {
            if req.session.is_some() { Err("a card session can't control sessions".to_string()) } else { crate::control::call(app, name, &body) }
        } else {
            tools::call(app, req.session.as_deref(), name, &body)
        } {
            Ok(v) => respond(&stream, "200 OK", &json!({ "ok": true, "result": v })),
            Err(e) => respond(&stream, "200 OK", &json!({ "ok": false, "error": e })),
        },
        _ => respond(&stream, "404 Not Found", &json!({ "error": "no such route" })),
    }
}

const INSTRUCTIONS: &str = "This session belongs to a card on Hugh's board, the Claude Kanban DSH app. \
Read your card with kanban_get_card and keep it current with kanban_update_card and kanban_add_log. \
A move of your own card waits until your turn ends. \
To show Hugh a screenshot in the card's chat (a test passing, the UI you changed), put it in your message as a \
Markdown image with its full path in forward slashes, for example ![what it shows](C:/path/to/shot.png). \
Never move a card into Ship it: only Hugh does that.";

const EXTERNAL_INSTRUCTIONS: &str = "Hugh's board, the Claude Kanban DSH desktop app. Read and change cards with the kanban_* card tools, \nand run the cards' Claude Code sessions with kanban_session_*: start (with a first message), send, status, read, \nswitch model or effort, stop, and keys for a prompt a session is stuck on. Starts over Hugh's running-sessions cap wait \nin the queue; never change the cap. Never move a card into Ship it: only Hugh does that.";

/// One JSON-RPC message. Notifications (no id) get no reply.
fn mcp(app: &AppHandle, caller: Option<&str>, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": msg.pointer("/params/protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "kanban", "version": env!("CARGO_PKG_VERSION") },
            "instructions": if caller.is_some() { INSTRUCTIONS } else { EXTERNAL_INSTRUCTIONS },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => {
            let mut list = tools::schemas();
            // Session control is for callers outside a card session only.
            if caller.is_none() {
                if let (Some(a), Some(b)) = (list.as_array_mut(), crate::control::schemas().as_array()) {
                    a.extend(b.iter().cloned());
                }
            }
            Ok(json!({ "tools": list }))
        }
        "tools/call" => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = msg.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            // `kb`-only tools aren't offered over MCP, and aren't run if a client asks anyway.
            let res = if tools::CLI_ONLY.contains(&name) || (caller.is_some() && crate::control::CONTROL.contains(&name)) {
                Err(format!("no tool called {name}"))
            } else if crate::control::CONTROL.contains(&name) {
                crate::control::call(app, name, &args)
            } else {
                tools::call(app, caller, name, &args)
            };
            Ok(match res {
                Ok(v) => json!({ "content": [{ "type": "text", "text": text_of(&v) }] }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            })
        }
        _ => Err(json!({ "code": -32601, "message": format!("no method {method}") })),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// One line per request (method, route with the token hidden, body size) in api.log.
fn log_request(req: &Request) {
    let dir = kanban_store::data_dir();
    let mut parts: Vec<&str> = req.path.split('/').collect();
    if parts.len() > 2 {
        parts[2] = "…";
    }
    let line = format!("{} {} {} {}b\n", chrono::Utc::now().to_rfc3339(), req.method, parts.join("/"), req.body.len());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("api.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}
