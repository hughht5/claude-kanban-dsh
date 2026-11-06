// The card panel's commands: a card's session and its transcript. Included in main.rs.

/// Where hooks, MCP and `kb` reach this run of the app.
pub struct Api(pub Option<api::ApiInfo>);

/// The conversation a card's chat view shows, and its account: the app's latest session
/// for the card, else the session the Work Board recorded.
fn conversation(store: &Store, card: &Card) -> Option<(String, String, &'static str)> {
    if let Ok(Some(row)) = store.latest_session(&card.id) {
        return Some((row.claude_session_id, row.account, "app"));
    }
    let id = card.fields.get("claudeSessionId").and_then(Value::as_str)?;
    Some((id.to_string(), sessions::card_account(card), "desktop"))
}

/// The model, effort and permission mode for a card: its overrides, else its column's settings.
fn session_settings(store: &Store, card: &Card) -> (String, String, String) {
    let cfg = store.column(&card.column).ok().flatten();
    let (dm, de) = sessions::column_defaults(&card.column);
    let pick = |over: &str, col: Option<String>, default: &str| {
        card.fields.get(over).and_then(Value::as_str).map(str::to_string).or(col).unwrap_or_else(|| default.to_string())
    };
    (
        pick("modelOverride", cfg.as_ref().and_then(|c| c.model.clone()), dm),
        pick("effortOverride", cfg.as_ref().and_then(|c| c.effort.clone()), de),
        pick("permissionOverride", cfg.as_ref().and_then(|c| c.permission_mode.clone()), "bypassPermissions"),
    )
}

#[tauri::command]
fn session_info(board: State<Board>, live: State<Arc<sessions::Sessions>>, card_id: String) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(&card_id).map_err(CmdError::other)?;
    let running = live.is_live(&card_id);
    let latest = store.latest_session(&card_id).map_err(CmdError::other)?;
    let convo = conversation(&store, &card);
    let (model, effort, _) = session_settings(&store, &card);
    let waiting_text = store.waiting(&card_id, "message").ok().flatten().map(|m| m.payload);
    let waiting = waiting_text.is_some();
    let (typing, untaken) = live.typing(&card_id);
    let queued = store.start_queue().ok().and_then(|q| q.position(&card_id));
    let cap = store.session_cap().unwrap_or(kanban_store::DEFAULT_SESSION_CAP);
    Ok(json!({
        "running": running,
        "state": live.states().get(&card_id),
        // How long the session has been on its current turn (ms), for the "Working…" line.
        "turnMs": live.turn_ms(&card_id),
        // The question dialog on screen (its tool_use_id), which the chat can answer.
        "question": live.question_id(&card_id),
        "latest": latest,
        "waiting": waiting,
        // For the chat's queued bubble: what waits to be typed (the app's text, such as a
        // column prompt, goes in with Hugh's), what was typed and isn't taken yet, and what
        // the session didn't take after its tries.
        "waitingText": waiting_text,
        "typing": typing,
        "untaken": untaken,
        "typeTries": sessions::TYPE_TRIES,
        "btw": live.btws(&card_id),
        "screen": live.on_screen(&card_id),
        "conversation": convo.as_ref().map(|c| json!({ "id": c.0, "account": c.1, "source": c.2 })),
        "account": convo.as_ref().map(|c| c.1.clone()).unwrap_or_else(|| dispatch::pick_account(&store, &card).0),
        "model": model,
        "effort": effort,
        "claude": sessions::claude_exe().map(|p| p.display().to_string()),
        "claudeVersion": sessions::claude_exe_version(),
        // A model or effort change waiting to be typed into the running session.
        "switch": live.pending_switch(&card_id),
        // The place in line of a start waiting under the cap, and the cap.
        "queued": queued,
        "cap": cap,
    }))
}

#[tauri::command]
fn session_states(live: State<Arc<sessions::Sessions>>) -> Value {
    json!(live.states())
}

/// The cards whose session is running a review or a test, for the "Testing" chip (card #86).
#[tauri::command]
fn session_testing(live: State<Arc<sessions::Sessions>>) -> Value {
    json!(live.testing_cards())
}

/// Starts a session for `card`. `mode` is "new" (a fresh conversation in the repo's folder)
/// or "resume" (the card's conversation, in the folder it ran in, on its own account).
pub(crate) fn start_session(app: &AppHandle, store: &mut Store, card: &Card, mode: &str) -> Result<SessionRow, String> {
    let live = app.state::<Arc<sessions::Sessions>>();
    let api = app.state::<Api>().0.clone();
    let (model, effort, permission_mode) = session_settings(store, card);
    let base = board_path().parent().map(|p| p.join("sessions")).unwrap_or_default();
    let row_id = uuid::Uuid::new_v4().to_string();
    let mut why = String::new();
    let plan = match mode {
        "new" => {
            let cwd = card
                .fields
                .get("worktreePath")
                .and_then(Value::as_str)
                .filter(|p| std::path::Path::new(p).is_dir())
                .map(PathBuf::from)
                .unwrap_or_else(|| sessions::repo_folder(card.fields.get("repo").and_then(Value::as_str)));
            let (account, reason) = dispatch::pick_account(store, card);
            why = reason;
            sessions::Plan {
                resume: None,
                settings_path: base.join(&row_id).join("settings.json"),
                session_id: uuid::Uuid::new_v4().to_string(),
                account,
                cwd: cwd.display().to_string(),
                model,
                effort,
                permission_mode,
                row_id: row_id.clone(),
                api,
            }
        }
        "resume" => {
            let (id, account, source) = conversation(store, card).ok_or("this card has no conversation to resume")?;
            let Some(path) = transcript::find(&account, &id) else {
                // Claude writes a transcript only once a conversation has a message. An app
                // session that never got one has nothing to resume, so it starts again.
                if source == "app" {
                    return start_session(app, store, card, "new");
                }
                return Err("its transcript isn't on this machine".into());
            };
            // Account 2's projects folder is a junction to the main one, so the transcript's
            // place says nothing about its account: the session row (or the card) does.
            // Claude itself resumes any conversation on either account, so the app refuses.
            if let Some(pin) = card.fields.get("accountPin").and_then(Value::as_str) {
                if pin != account {
                    return Err(format!(
                        "this conversation runs on {}, but the card is pinned to {}; start a new session to use {}",
                        acct_name(&account), acct_name(pin), acct_name(pin)
                    ));
                }
            }
            let cwd = transcript::cwd_of(&path)
                .filter(|p| std::path::Path::new(p).is_dir())
                .ok_or("the folder this conversation ran in no longer exists, so Claude can't resume it")?;
            sessions::Plan {
                resume: Some(id.clone()),
                settings_path: base.join(&row_id).join("settings.json"),
                session_id: id,
                account,
                cwd,
                model,
                effort,
                permission_mode,
                row_id: row_id.clone(),
                api,
            }
        }
        _ => return Err("mode must be new or resume".into()),
    };
    let row = live.start(app, card, &plan)?;
    store.add_session(&row).map_err(|e| e.to_string())?;
    let text = format!(
        "{} a session in the app on {}{} ({} {}, {})",
        if plan.resume.is_some() { "Resumed" } else { "Started" },
        acct_name(&row.account),
        if why.is_empty() { String::new() } else { format!(" ({why})") },
        row.model.clone().unwrap_or_default(),
        row.effort.clone().unwrap_or_default(),
        row.cwd
    );
    let _ = store.add_log(&card.id, &text, "the app");
    if plan.resume.is_none() {
        if store.waiting(&card.id, "message").ok().flatten().is_none() {
            // A new session learns which card it is for, and its template's starting prompt.
            // A move's message (the column prompt) is added after this one.
            let first = dispatch::dry(card, dispatch::opening(store, card));
            let _ = store.queue(&card.id, "message", &first);
        } else if let Some(start) = dispatch::template_prompt(store, card) {
            let _ = store.queue(&card.id, "message", &start);
        }
    }
    let _ = app.emit("session-changed", card.id.clone());
    Ok(row)
}

#[tauri::command]
fn session_start(app: AppHandle, board: State<Board>, card_id: String, mode: String) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(&card_id).map_err(CmdError::other)?;
    let begun = begin_session(&app, &mut store, &card, &mode, "hugh").map_err(CmdError::other)?;
    drop(store);
    changed(&app);
    Ok(match begun {
        Begun::Started(row) => json!(row),
        Begun::Queued(pos) => json!({ "queued": pos }),
    })
}

#[tauri::command]
fn session_input(live: State<Arc<sessions::Sessions>>, card_id: String, data: String) -> CmdResult<()> {
    live.input(&card_id, data.as_bytes()).map_err(CmdError::other)
}

#[tauri::command]
fn session_resize(live: State<Arc<sessions::Sessions>>, card_id: String, cols: u16, rows: u16) -> CmdResult<()> {
    live.resize(&card_id, cols, rows).map_err(CmdError::other)
}

#[tauri::command]
fn session_snapshot(live: State<Arc<sessions::Sessions>>, card_id: String) -> Option<String> {
    live.snapshot(&card_id)
}

/// A slash command from the chat never takes a Human test card back to work (`back_to_work`
/// would wrap it in a note, and it would no longer be a command): it goes in as typed.
///
/// A slash command from the chat that isn't typed in as a message: `/btw` is asked and answered
/// in the chat (`session_btw`), and `/exit` or `/quit` is refused (the app would take the
/// session's end for a crash; Stop session pauses it so it can be resumed). None for anything
/// else, which is sent as usual. Neither moves the card (`back_to_work`).
fn chat_command(app: &AppHandle, card_id: &str, text: &str) -> Option<CmdResult<()>> {
    if let Some(q) = btw::question(text) {
        let live = app.state::<Arc<sessions::Sessions>>();
        return Some(session_btw(app.clone(), live, card_id.to_string(), q.to_string()));
    }
    let first = text.trim_start().split_whitespace().next().unwrap_or("");
    if first == "/exit" || first == "/quit" {
        return Some(Err(CmdError::other("not from the chat: use Stop session, which pauses the session so it can be resumed")));
    }
    None
}

/// Hugh's `/btw <question>` from the chat: asked in the running session, and its answer kept
/// for the chat (`Sessions::ask_btw`). The chat hears "session-changed" when it is asked and
/// again when it is answered.
#[tauri::command]
fn session_btw(app: AppHandle, live: State<Arc<sessions::Sessions>>, card_id: String, question: String) -> CmdResult<()> {
    let (a, id) = (app.clone(), card_id.clone());
    live.ask_btw(&card_id, &question, move || {
        let _ = a.emit("session-changed", id.clone());
    })
    .map_err(CmdError::other)?;
    let _ = app.emit("session-changed", card_id);
    Ok(())
}

/// Claude Code's own command list per account and folder (`slash::claude_list`), asked once
/// per run of the app: None while it is being asked, or after it failed.
static CLAUDE_LISTS: std::sync::OnceLock<Mutex<std::collections::HashMap<(String, String), Option<slash::ClaudeList>>>> = std::sync::OnceLock::new();

/// The chat's `/` menu for a card, for the account and folder its session runs (or would run)
/// in: Claude Code's own list (`slash::with_claude`) once it has been asked, which the first
/// call starts in the background; until then the built-ins and what is on disk (`slash::list`),
/// with `complete` false so the panel asks again.
#[tauri::command]
fn slash_commands(board: State<Board>, card_id: String) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(&card_id).map_err(CmdError::other)?;
    let row = store.latest_session(&card_id).ok().flatten();
    let account = row.as_ref().map(|r| r.account.clone()).unwrap_or_else(|| sessions::card_account(&card));
    let cwd = row.map(|r| r.cwd).or_else(|| card.fields.get("worktreePath").and_then(Value::as_str).map(str::to_string));
    let number = card.number;
    drop(store);
    let path = cwd.as_deref().map(std::path::Path::new);
    let disk = slash::list(&transcript::config_dir(&account), path);
    let key = (account.clone(), cwd.clone().unwrap_or_default());
    let lists = CLAUDE_LISTS.get_or_init(Default::default);
    let mut map = lists.lock().map_err(CmdError::other)?;
    match map.get(&key) {
        Some(Some(l)) => return Ok(json!({ "items": slash::with_claude(l, &disk), "complete": true })),
        Some(None) => {}
        None => {
            map.insert(key.clone(), None);
            if let Some(exe) = sessions::claude_exe() {
                let env = sessions::build_env(std::env::vars(), &account, number);
                std::thread::spawn(move || {
                    let got = slash::claude_list(&exe, &env, cwd.as_deref().map(std::path::Path::new));
                    if let Ok(mut m) = lists.lock() {
                        // A failure is forgotten, so the next menu asks again.
                        match got {
                            Some(l) => {
                                m.insert(key, Some(l));
                            }
                            None => {
                                m.remove(&key);
                            }
                        }
                    }
                });
            }
        }
    }
    Ok(json!({ "items": disk, "complete": false }))
}

/// What the card session's terminal shows now, as text (`Sessions::screen_text`).
#[tauri::command]
fn session_screen(live: State<Arc<sessions::Sessions>>, card_id: String) -> Option<String> {
    live.screen_text(&card_id)
}

/// Sends `text` to the card's session as Hugh's message. A running session gets it at once,
/// as if he typed it in the terminal, even mid-turn (`Sessions::type_now`), together with
/// anything already waiting for it; it waits in the queue only while the session is starting
/// or asking him something. A paused (or never started) session comes back for it: resumed
/// if it has a conversation in the app, else started new (its header goes first, then this
/// message). Over the cap it waits in the queue, and the message waits with it. A
/// conversation that ran in the desktop app isn't started: the message waits for Hugh's
/// resume. Returns whether a session was started (or queued to start). The caller emits
/// "session-changed".
fn send_to_session(app: &AppHandle, store: &mut Store, card_id: &str, text: &str) -> Result<bool, String> {
    if text.trim().is_empty() {
        return Err("there's nothing to send".into());
    }
    let live = app.state::<Arc<sessions::Sessions>>();
    // The chat's Send again: the message the session didn't take, sent as a new message.
    live.clear_untaken(card_id, text);
    if live.is_live(card_id).is_some() {
        // Something already waiting goes in with it, and only at the prompt, never mid-turn:
        // it may be the app's own text (a column prompt, a notice), which waits for `free`.
        let alone = store.waiting(card_id, "message").ok().flatten().is_none();
        // Queued first, so it is on record and joins (after) anything already waiting.
        store.queue(card_id, "message", text).map_err(|e| e.to_string())?;
        // A held move goes first, as the dispatcher does it: the message waits behind it.
        if store.waiting(card_id, "move").ok().flatten().is_some() {
            return Ok(false);
        }
        if let Ok(Some(m)) = store.waiting(card_id, "message") {
            // A failed write leaves it queued: the dispatcher types it once the session is free.
            if live.type_now(card_id, &m.payload, alone).unwrap_or(false) {
                let _ = store.done(m.id, "sent");
            }
        }
        return Ok(false);
    }
    let mut started = false;
    if live.is_live(card_id).is_none() && store.start_queue().map(|q| q.position(card_id).is_none()).unwrap_or(true) {
        let card = store.get(card_id).map_err(|e| e.to_string())?;
        let desktop_only = store.latest_session(card_id).ok().flatten().is_none() && card.fields.get("claudeSessionId").is_some();
        if !desktop_only {
            let mode = if store.latest_session(card_id).ok().flatten().is_some() { "resume" } else { "new" };
            begin_session(app, store, &card, mode, "message")?;
            started = true;
        }
    }
    store.queue(card_id, "message", text).map_err(|e| e.to_string())?;
    Ok(started)
}

/// Hugh's message to a card in Human test takes it back to In progress first (card #93): moved
/// as his own move (so its test is needed again), logged, and `text` comes back with a note
/// for the session. Any other card's message is unchanged. Returns the text and whether the
/// card moved.
fn back_to_work(store: &mut Store, card_id: &str, text: &str) -> CmdResult<(String, bool)> {
    let card = store.get(card_id).map_err(CmdError::other)?;
    if text.trim().is_empty() || !dispatch::message_moves_back(&card) {
        return Ok((text.to_string(), false));
    }
    let m = store.move_by_hugh(card_id, card.version, "In progress", Patch::new()).map_err(|e| refused(store, e))?;
    let _ = store.add_log(card_id, "Hugh's message took it back to In progress", HUGH);
    Ok((dispatch::moved_back_message(&m.card, text), true))
}

/// Hugh's message from the panel (`send_to_session`).
#[tauri::command]
fn session_send(app: AppHandle, board: State<Board>, card_id: String, text: String) -> CmdResult<()> {
    if let Some(r) = chat_command(&app, &card_id, &text) {
        return r;
    }
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let (text, moved) = if sessions::confirms(&text) { back_to_work(&mut store, &card_id, &text)? } else { (text, false) };
    let sent = send_to_session(&app, &mut store, &card_id, &text);
    if let (true, Err(e)) = (moved, &sent) {
        let _ = store.add_log(&card_id, &format!("Couldn't send Hugh's message to the session: {e}"), "the app");
        changed(&app);
    }
    let started = sent.map_err(CmdError::other)?;
    drop(store);
    if started || moved {
        changed(&app);
    }
    let _ = app.emit("session-changed", card_id);
    Ok(())
}

/// The chat's Dismiss on a message the session didn't take: forgets it unsent.
#[tauri::command]
fn session_dismiss_untaken(app: AppHandle, live: State<Arc<sessions::Sessions>>, card_id: String, text: String) {
    live.clear_untaken(&card_id, &text);
    let _ = app.emit("session-changed", card_id);
}

/// Hugh's answer to the session's question dialog, from the chat's question card: typed into
/// the dialog as its keys (`Sessions::answer`). Refused unless `tool_id` is the question on
/// screen; the panel then says to answer in the terminal.
#[tauri::command]
fn session_answer(app: AppHandle, live: State<Arc<sessions::Sessions>>, card_id: String, tool_id: String, answers: Vec<sessions::Answer>) -> CmdResult<()> {
    live.answer(&card_id, &tool_id, &answers).map_err(CmdError::other)?;
    let _ = app.emit("session-changed", card_id);
    Ok(())
}

/// Hugh's "send now" (Ctrl+X Ctrl+S or Ctrl+Enter in the panel's box, or Send now on a
/// waiting message), as in the terminal: mid-turn, `text` is typed and Claude Code's send-now
/// keys interrupt the turn and send it, with anything it had queued. Empty `text` sends what
/// it has queued. Not mid-turn, or while anything of the app's is waiting, it is an ordinary
/// Send (`send_to_session`).
#[tauri::command]
fn session_send_now(app: AppHandle, board: State<Board>, card_id: String, text: String) -> CmdResult<()> {
    if let Some(r) = chat_command(&app, &card_id, &text) {
        return r;
    }
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let (text, moved) = if sessions::confirms(&text) { back_to_work(&mut store, &card_id, &text)? } else { (text, false) };
    if moved {
        changed(&app);
    }
    let live = app.state::<Arc<sessions::Sessions>>();
    let quiet_queue = store.waiting(&card_id, "message").ok().flatten().is_none() && store.waiting(&card_id, "move").ok().flatten().is_none();
    if text.trim().is_empty() {
        if quiet_queue {
            live.send_now(&card_id, "").map_err(CmdError::other)?;
        }
        return Ok(());
    }
    if quiet_queue && live.states().get(&card_id).map(String::as_str) == Some("working") {
        let id = store.queue(&card_id, "message", &text).map_err(CmdError::other)?;
        if live.send_now(&card_id, &text).unwrap_or(false) {
            let _ = store.done(id, "sent now");
            drop(store);
            let _ = app.emit("session-changed", card_id);
            return Ok(());
        }
        // It couldn't go now: it stays queued, and goes in as an ordinary message.
        drop(store);
        let _ = app.emit("session-changed", card_id);
        return Ok(());
    }
    let sent = send_to_session(&app, &mut store, &card_id, &text);
    if let (true, Err(e)) = (moved, &sent) {
        let _ = store.add_log(&card_id, &format!("Couldn't send Hugh's message to the session: {e}"), "the app");
    }
    let started = sent.map_err(CmdError::other)?;
    drop(store);
    if started {
        changed(&app);
    }
    let _ = app.emit("session-changed", card_id);
    Ok(())
}

#[tauri::command]
async fn session_stop(app: AppHandle, card_id: String) -> CmdResult<()> {
    let live = app.state::<Arc<sessions::Sessions>>().inner().clone();
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || live.stop(&a, &card_id))
        .await
        .map_err(CmdError::other)?
        .map_err(CmdError::other)
}

#[tauri::command]
fn transcript_read(board: State<Board>, card_id: String, offset: u64, agent: Option<String>) -> CmdResult<transcript::Chunk> {
    let empty = || transcript::Chunk { entries: vec![], offset: 0, path: None, thought: None };
    let Some(path) = card_transcript(&board, &card_id)? else { return Ok(empty()) };
    match agent.filter(|a| !a.is_empty()) {
        None => transcript::read(&path, offset).map_err(CmdError::other),
        Some(a) => match transcript::subagent_path(&path, &a) {
            Some(p) => transcript::read_in(&p, offset, true).map_err(CmdError::other),
            None => Err(CmdError::other("that subagent's transcript isn't on this machine")),
        },
    }
}

/// The card conversation's transcript, if it is on this machine.
fn card_transcript(board: &State<Board>, card_id: &str) -> CmdResult<Option<std::path::PathBuf>> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(card_id).map_err(CmdError::other)?;
    let Some((id, account, _)) = conversation(&store, &card) else { return Ok(None) };
    drop(store);
    Ok(transcript::find(&account, &id))
}

/// The subagents the card's conversation has run, oldest first, for the chat's switcher.
#[tauri::command]
async fn subagent_list(board: State<'_, Board>, card_id: String) -> CmdResult<Vec<transcript::Subagent>> {
    let path = card_transcript(&board, &card_id)?;
    tauri::async_runtime::spawn_blocking(move || path.map(|p| transcript::subagents(&p)).unwrap_or_default())
        .await
        .map_err(CmdError::other)
}

fn acct_name(a: &str) -> &'static str {
    if a == "account2" { "Account 2" } else { "the main account" }
}

// The session footer: the latest statusLine report for each session row, in memory only.
// `dispatch::on_status` hands every report to `note_status`.

fn status_snaps() -> &'static std::sync::Mutex<std::collections::HashMap<String, Value>> {
    static SNAPS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Value>>> = std::sync::OnceLock::new();
    SNAPS.get_or_init(Default::default)
}

/// The parts of a statusLine report the footer shows. Every field may be missing: older
/// Claude Code versions send no context window, and before the first reply there is no usage.
fn status_snapshot(body: &Value) -> Value {
    let f = |p: &str| body.pointer(p).and_then(Value::as_f64);
    let s = |p: &str| body.pointer(p).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    let size = f("/context_window/context_window_size").filter(|v| *v > 0.0);
    let used = body
        .pointer("/context_window/current_usage")
        .filter(|u| u.is_object())
        .map(|u| {
            ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
                .iter()
                .filter_map(|k| u.get(*k).and_then(Value::as_f64))
                .sum::<f64>()
        });
    let pct = f("/context_window/used_percentage").or(match (used, size) {
        (Some(u), Some(s)) => Some(u / s * 100.0),
        _ => None,
    });
    json!({
        "model": s("/model/display_name").or_else(|| s("/model/id")).or_else(|| s("/model")),
        "modelId": s("/model/id"),
        "version": s("/version"),
        "costUsd": f("/cost/total_cost_usd"),
        "durationMs": f("/cost/total_duration_ms"),
        "apiDurationMs": f("/cost/total_api_duration_ms"),
        "linesAdded": f("/cost/total_lines_added"),
        "linesRemoved": f("/cost/total_lines_removed"),
        "contextPct": pct,
        "contextTokens": used,
        "contextSize": size,
        "exceeds200k": body.get("exceeds_200k_tokens").and_then(Value::as_bool),
        "at": chrono::Utc::now().to_rfc3339(),
    })
}

/// Keeps a session's latest statusLine report and tells the panel.
pub(crate) fn note_status(app: &AppHandle, row_id: &str, body: &Value) {
    if let Ok(mut m) = status_snaps().lock() {
        m.insert(row_id.to_string(), status_snapshot(body));
    }
    let _ = app.emit("session-status", row_id.to_string());
}

/// The footer's figures for a card's latest session: its last statusLine report, if any.
#[tauri::command]
fn session_status(board: State<Board>, live: State<Arc<sessions::Sessions>>, card_id: String) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let latest = store.latest_session(&card_id).map_err(CmdError::other)?;
    drop(store);
    let running = live.is_live(&card_id);
    let row = running.or(latest);
    let snap = row.as_ref().and_then(|r| status_snaps().lock().ok()?.get(&r.id).cloned());
    Ok(json!({
        "row": row.as_ref().map(|r| r.id.clone()),
        "running": live.is_live(&card_id).is_some(),
        "startedAt": row.as_ref().map(|r| r.started_at.clone()),
        "model": row.as_ref().and_then(|r| r.model.clone()),
        "status": snap,
    }))
}

// Attachments: a pasted screenshot or a dropped file is saved under the app's data folder, so
// the message can carry its path for Claude to Read. The window's own drop handling is off
// (it broke the board's card drag), so a dropped file reaches the page as bytes with no path.

const MAX_ATTACHMENT: usize = 25 * 1024 * 1024;

/// A file or folder name made of safe characters only, at most 80 of them.
fn safe_name(s: &str, fallback: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' })
        .collect();
    let t: String = t.trim_matches(|c| c == '.' || c == '-').chars().take(80).collect();
    if t.is_empty() { fallback.to_string() } else { t }
}

/// A new file in `dir` for `name`: `<stem>-<time>[-n].<ext>`, never an existing one.
fn attachment_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let name = safe_name(name, "shot.png");
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut i = 1;
    loop {
        let n = if i == 1 { String::new() } else { format!("-{i}") };
        let p = dir.join(format!("{stem}-{stamp}{n}{ext}"));
        if !p.exists() {
            return p;
        }
        i += 1;
    }
}

fn save_attachment(root: &std::path::Path, card_id: &str, name: &str, data: &str) -> Result<PathBuf, String> {
    use base64::Engine as _;
    let b64 = data.split_once("base64,").map_or(data, |(_, d)| d);
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| format!("not a file: {e}"))?;
    if bytes.is_empty() {
        return Err("the file is empty".into());
    }
    if bytes.len() > MAX_ATTACHMENT {
        return Err(format!("the file is over {} MB", MAX_ATTACHMENT / 1024 / 1024));
    }
    let dir = root.join("attachments").join(safe_name(card_id, "card"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = attachment_path(&dir, name);
    std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Saves a pasted image or dropped file for `card_id` and returns its absolute path.
#[tauri::command]
fn attachment_save(card_id: String, name: String, data: String) -> CmdResult<String> {
    let p = save_attachment(&kanban_store::data_dir(), &card_id, &name, &data).map_err(CmdError::other)?;
    Ok(p.display().to_string())
}

// Screenshots in the chat: the page's CSP lets images come only from itself or `data:` URLs,
// so a local image a message shows (a Markdown image, a Read screenshot, an attachment) is
// read here and handed back as a data URL. Only image files, and none over the attachment cap.

fn image_data_url(path: &str) -> Result<String, String> {
    use base64::Engine as _;
    let path = match path.get(..8) {
        Some(head) if head.eq_ignore_ascii_case("file:///") => &path[8..],
        _ => path,
    };
    // A local drive only: a network path (\\host\share) would have Windows connect to that host.
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Err("not a local file".into());
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("not a full path".into());
    }
    let mime = crate::transcript::image_mime(path).ok_or("not an image")?;
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > MAX_ATTACHMENT as u64 {
        return Err(format!("the image is over {} MB", MAX_ATTACHMENT / 1024 / 1024));
    }
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// A local image file as a `data:` URL, for the chat to draw.
#[tauri::command]
fn image_read(path: String) -> CmdResult<String> {
    image_data_url(&path).map_err(CmdError::other)
}

#[cfg(test)]
mod panel_cmd_tests {
    use super::*;

    #[test]
    fn image_read_hands_back_images_only() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("shot.png");
        std::fs::write(&png, b"PNG-ish bytes").unwrap();
        let url = image_data_url(&png.display().to_string()).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        let file_url = format!("file:///{}", png.display().to_string().replace('\\', "/"));
        assert_eq!(image_data_url(&file_url).unwrap(), url);
        // Not an image, not there, a folder, or not a full path: refused.
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, b"secret").unwrap();
        assert!(image_data_url(&txt.display().to_string()).is_err());
        assert!(image_data_url(&dir.path().join("gone.png").display().to_string()).is_err());
        let folder = dir.path().join("d.png");
        std::fs::create_dir(&folder).unwrap();
        assert!(image_data_url(&folder.display().to_string()).is_err());
        assert!(image_data_url("shot.png").is_err());
        assert_eq!(image_data_url(&file_url.replacen("file", "FILE", 1)).unwrap(), url);
        // Never a network path.
        assert_eq!(image_data_url(r"\\host\share\a.png").unwrap_err(), "not a local file");
        assert_eq!(image_data_url("//host/share/a.png").unwrap_err(), "not a local file");
    }

    #[test]
    fn status_snapshot_reads_what_is_there() {
        let full = json!({
            "model": { "id": "claude-opus-4-1", "display_name": "Opus" },
            "version": "2.0.40",
            "cost": { "total_cost_usd": 1.25, "total_duration_ms": 65000, "total_lines_added": 3 },
            "context_window": {
                "context_window_size": 200000,
                "current_usage": { "input_tokens": 10000, "cache_creation_input_tokens": 20000, "cache_read_input_tokens": 20000, "output_tokens": 999 }
            }
        });
        let s = status_snapshot(&full);
        assert_eq!(s["model"], "Opus");
        assert_eq!(s["version"], "2.0.40");
        assert_eq!(s["costUsd"], 1.25);
        assert_eq!(s["durationMs"], 65000.0);
        assert_eq!(s["contextTokens"], 50000.0);
        assert_eq!(s["contextPct"], 25.0);
        // A reported percentage wins over the computed one.
        let given = json!({ "context_window": { "used_percentage": 42, "context_window_size": 200000, "current_usage": { "input_tokens": 1 } } });
        assert_eq!(status_snapshot(&given)["contextPct"], 42.0);
        // Missing pieces come back null, never an error.
        let bare = status_snapshot(&json!({ "model": "sonnet", "context_window": { "current_usage": null } }));
        assert_eq!(bare["model"], "sonnet");
        assert!(bare["costUsd"].is_null() && bare["contextPct"].is_null() && bare["version"].is_null());
        assert!(status_snapshot(&json!({}))["model"].is_null());
    }

    #[test]
    fn attachments_save_under_the_card_with_safe_unique_names() {
        use base64::Engine as _;
        let root = tempfile::tempdir().unwrap();
        let data = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b"\x89PNG fake"));
        let a = save_attachment(root.path(), "card/../x", "my shot?.png", &data).unwrap();
        let b = save_attachment(root.path(), "card/../x", "my shot?.png", &data).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.parent().unwrap(), root.path().join("attachments").join("card-..-x"));
        let name = a.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("my-shot-") && name.ends_with(".png"), "{name}");
        assert_eq!(std::fs::read(&a).unwrap(), b"\x89PNG fake");
        assert!(save_attachment(root.path(), "c", "x.png", "").is_err());
        assert!(save_attachment(root.path(), "c", "x.png", "!!!").is_err());
        assert_eq!(safe_name("..", "shot.png"), "shot.png");
        // A dropped non-image file keeps its own name and extension.
        let pdf = format!("data:application/pdf;base64,{}", base64::engine::general_purpose::STANDARD.encode(b"%PDF-1"));
        let c = save_attachment(root.path(), "c", "Q3 report.pdf", &pdf).unwrap();
        let n = c.file_name().unwrap().to_str().unwrap();
        assert!(n.starts_with("Q3-report-") && n.ends_with(".pdf"), "{n}");
    }
}
