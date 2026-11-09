// Switching a running session's model and effort, and the cap on running sessions with the
// starts waiting under it. Included in main.rs, after session_cmds.rs.

/// What a request to start did.
pub(crate) enum Begun {
    Started(SessionRow),
    /// Waiting under the cap, at this place in line (from 1).
    Queued(usize),
    /// Waiting for the app to size the card first (card #159); it starts once that is done.
    Sizing,
}

/// Every start goes through here: it starts at once when fewer sessions than the cap
/// (`sessions.max_running`) are running and no other card is waiting, and otherwise waits in
/// the queue and starts oldest-first when a place comes free. `why` is "hugh", "move" or
/// "restart".
pub(crate) fn begin_session(app: &AppHandle, store: &mut Store, card: &Card, mode: &str, why: &str) -> Result<Begun, String> {
    if mode != "new" && mode != "resume" {
        return Err("mode must be new or resume".into());
    }
    if app.state::<Arc<sessions::Sessions>>().is_live(&card.id).is_some() {
        return Err("this card already has a running session".into());
    }
    // Card #159: sized first, while Hugh's setting is on; the start waits for it.
    if presize::hold(app, store, card, mode, why) {
        return Ok(Begun::Sizing);
    }
    begin_session_now(app, store, card, mode, why)
}

/// `begin_session` without the sizing step: the start the sizing held, once it is done.
pub(crate) fn begin_session_now(app: &AppHandle, store: &mut Store, card: &Card, mode: &str, why: &str) -> Result<Begun, String> {
    if mode != "new" && mode != "resume" {
        return Err("mode must be new or resume".into());
    }
    let live = app.state::<Arc<sessions::Sessions>>();
    if live.is_live(&card.id).is_some() {
        return Err("this card already has a running session".into());
    }
    let cap = store.session_cap().map_err(|e| e.to_string())?;
    let q = store.start_queue().map_err(|e| e.to_string())?;
    let running = live.running_count();
    // A stack's lead is the integrator its children wait on, so its start goes first (card #113).
    let first = card.is_lead();
    if if first { q.starts_now_first(&card.id, running, cap) } else { q.starts_now(&card.id, running, cap) } {
        let was_queued = store.unqueue_start(&card.id).map_err(|e| e.to_string())?.is_some();
        let row = start_session(app, store, card, mode)?;
        if was_queued {
            emit_queue(app, store);
        }
        return Ok(Begun::Started(row));
    }
    if mode == "resume" && conversation(store, card).is_none() {
        return Err("this card has no conversation to resume".into());
    }
    let pos = if first { store.queue_start_first(&card.id, mode, why) } else { store.queue_start(&card.id, mode, why) }.map_err(|e| e.to_string())?;
    let _ = store.add_log(
        &card.id,
        &format!(
            "Queued to start ({}): {running} sessions are running and the cap is {cap}. It starts when one ends; it is number {pos} in line.",
            if mode == "resume" { "resume" } else { "new session" }
        ),
        "the app",
    );
    emit_queue(app, store);
    let _ = app.emit("session-changed", card.id.clone());
    Ok(Begun::Queued(pos))
}

/// The cap and the waiting starts, for the board's "Queued (cap N)" badges.
fn queue_json(store: &Store, running: usize) -> Value {
    let cap = store.session_cap().unwrap_or(kanban_store::DEFAULT_SESSION_CAP);
    let q = store.start_queue().unwrap_or_default();
    let queued: Vec<Value> = q
        .items
        .iter()
        .enumerate()
        .map(|(i, w)| json!({ "card": w.card_id, "position": i + 1, "mode": w.mode, "why": w.why, "queuedAt": w.queued_at }))
        .collect();
    json!({ "cap": cap, "running": running, "queued": queued })
}

/// Tells the page the queue changed (event `session-queue`, with `queue_json`'s value).
fn emit_queue(app: &AppHandle, store: &Store) {
    let running = app.state::<Arc<sessions::Sessions>>().running_count();
    let _ = app.emit("session-queue", queue_json(store, running));
}

/// Starts waiting sessions, oldest first, while there are places under the cap. The
/// dispatcher calls it every tick, so a place freed by a session ending, crashing or being
/// suspended, or by a higher cap, is taken within half a second.
pub(crate) fn drain_queue(app: &AppHandle) {
    let live = app.state::<Arc<sessions::Sessions>>().inner().clone();
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let Ok(mut q) = store.start_queue() else { return };
    if q.is_empty() {
        return;
    }
    let cap = store.session_cap().unwrap_or(kanban_store::DEFAULT_SESSION_CAP);
    let due = q.take(live.running_count(), cap);
    if due.is_empty() {
        return;
    }
    // Saved first, so a start that fails (or a crash part-way) isn't tried again and again.
    if store.save_start_queue(&q).is_err() {
        return;
    }
    for w in due {
        let Ok(card) = store.get(&w.card_id) else { continue };
        let done = store.column(&card.column).ok().flatten().map_or(false, |c| c.on_enter == "suspend");
        if card.archived || done || live.is_live(&card.id).is_some() {
            let why = if card.archived { "it was archived" } else if done { "it is in a column that stops sessions" } else { "it is already running" };
            let _ = store.add_log(&card.id, &format!("Didn't start the queued session: {why}"), "the app");
            continue;
        }
        let prev = store.latest_session(&card.id).ok().flatten().map(|r| r.claude_session_id);
        match start_session(app, &mut store, &card, &w.mode) {
            Ok(row) => {
                let _ = store.add_log(&card.id, &format!("Started from the queue: a place came free under the cap of {cap}"), "the app");
                if w.why == "restart" {
                    dispatch::restarted(&mut store, &card, prev.as_deref(), &row);
                }
            }
            Err(e) => {
                let _ = store.add_log(&card.id, &format!("Couldn't start the queued session: {e}"), "the app");
            }
        }
    }
    emit_queue(app, &store);
    drop(store);
    changed(app);
}

/// The most sessions that run at once (the setting `sessions.max_running`, 10 when unset).
#[tauri::command]
fn get_session_cap(board: State<Board>) -> CmdResult<usize> {
    let store = board.0.lock().map_err(CmdError::other)?;
    store.session_cap().map_err(CmdError::other)
}

/// Sets the cap (1 to 100). A higher cap starts waiting sessions at once; a lower one stops
/// nothing that is running, but nothing more starts until the count is under it.
#[tauri::command]
fn set_session_cap(app: AppHandle, board: State<Board>, cap: usize) -> CmdResult<usize> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let cap = store.set_session_cap(cap).map_err(CmdError::other)?;
    emit_queue(&app, &store);
    drop(store);
    let a = app.clone();
    std::thread::spawn(move || drain_queue(&a));
    Ok(cap)
}

/// The cap, how many sessions run, and the starts waiting, in order.
#[tauri::command]
fn session_queue(board: State<Board>, live: State<Arc<sessions::Sessions>>) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    Ok(queue_json(&store, live.running_count()))
}

/// Takes the card's waiting start out of the queue. Returns whether it had one.
#[tauri::command]
fn session_unqueue(app: AppHandle, board: State<Board>, card_id: String) -> CmdResult<bool> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let had = store.unqueue_start(&card_id).map_err(CmdError::other)?.is_some();
    if had {
        let _ = store.add_log(&card_id, "Took the waiting start out of the queue", "Hugh (panel)");
        emit_queue(&app, &store);
    }
    drop(store);
    let _ = app.emit("session-changed", card_id);
    changed(&app);
    Ok(had)
}

/// Changes the model and/or effort of the card's session. Both are recorded on the card
/// (`modelOverride`, `effortOverride`), so every later start uses them. A running session gets
/// `/model <id>` and `/effort <level>` typed into it when it is next free (ready, idle and
/// quiet; after the current turn if it is mid-turn), so its conversation carries on.
#[tauri::command]
fn set_session_model_effort(
    app: AppHandle,
    board: State<Board>,
    live: State<Arc<sessions::Sessions>>,
    card_id: String,
    model: Option<String>,
    effort: Option<String>,
) -> CmdResult<Value> {
    let model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    let effort = effort.map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty());
    if model.is_none() && effort.is_none() {
        return Err(CmdError::other("choose a model or an effort"));
    }
    if let Some(m) = &model {
        sessions::model_command(m).map_err(CmdError::other)?;
    }
    if let Some(e) = &effort {
        sessions::effort_command(e).map_err(CmdError::other)?;
    }
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(&card_id).map_err(CmdError::other)?;
    let mut patch = Patch::new();
    if let Some(m) = &model {
        patch.insert("modelOverride".into(), json!(m));
    }
    if let Some(e) = &effort {
        patch.insert("effortOverride".into(), json!(e));
    }
    store.update(&card.id, card.version, &patch, "Hugh (panel)").map_err(|e| refused(&store, e))?;
    let running = live.is_live(&card_id).is_some();
    let waiting = if running { live.request_switch(&card_id, model.clone(), effort.clone()).ok() } else { None };
    let what = [model.as_ref().map(|m| format!("model {m}")), effort.as_ref().map(|e| format!("effort {e}"))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" and ");
    let text = match &waiting {
        Some(w) if !w.is_empty() => format!("Switching the running session to {what}: typed as /model or /effort when it is next free. Later starts use it too."),
        Some(_) => format!("The running session already uses {what}. Later starts use it too."),
        None => format!("Set {what} for this card's sessions: it applies when the next one starts."),
    };
    let _ = store.add_log(&card_id, &text, "Hugh (panel)");
    drop(store);
    changed(&app);
    let _ = app.emit("session-changed", card_id.clone());
    Ok(json!({ "running": running, "switch": waiting.filter(|w| !w.is_empty()), "message": text }))
}

/// The cards in the resume offer that can still be resumed: the store's offer (sessions idle
/// at the last quit or crash, until Hugh answers), less cards archived or in Done since, and
/// cards running or waiting to start now.
fn resume_offer_cards(app: &AppHandle, store: &Store) -> Vec<Card> {
    let live = app.state::<Arc<sessions::Sessions>>();
    let queue = store.start_queue().unwrap_or_default();
    store
        .resume_offer()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| store.get(&row.card_id).ok())
        .filter(|card| {
            let done = store.column(&card.column).ok().flatten().map_or(false, |c| c.on_enter == "suspend");
            !card.archived && !done && live.is_live(&card.id).is_none() && queue.position(&card.id).is_none()
        })
        .collect()
}

/// The resume offer the board's bar shows: `[{card, number, title, column}]`, oldest session first.
#[tauri::command]
fn resume_offer(app: AppHandle, board: State<Board>) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let cards: Vec<Value> = resume_offer_cards(&app, &store)
        .iter()
        .map(|c| json!({ "card": c.id, "number": c.number, "title": c.fields.get("title"), "column": c.column }))
        .collect();
    Ok(json!(cards))
}

/// Hugh's answer to the resume offer. `resume` true (Resume all) resumes each card's
/// conversation through `begin_session`, so over the cap the rest wait in the queue; false
/// (Not now) leaves them stopped. Either way nothing offered so far is offered again.
/// Returns how many were asked to resume.
#[tauri::command]
fn resume_offer_answer(app: AppHandle, board: State<Board>, resume: bool) -> CmdResult<usize> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let cards = if resume { resume_offer_cards(&app, &store) } else { vec![] };
    store.answer_resume_offer().map_err(CmdError::other)?;
    drop(store);
    let n = cards.len();
    let a = app.clone();
    // Each start takes the lock on its own, so the board stays responsive while they spawn.
    std::thread::spawn(move || {
        for card in cards {
            let board = a.state::<Board>();
            let Ok(mut store) = board.0.lock() else { return };
            let text = match begin_session(&a, &mut store, &card, "resume", "restart") {
                Ok(_) => "Resumed: it was running when the app closed (Resume all)".to_string(),
                Err(e) => format!("Couldn't resume (Resume all): {e}"),
            };
            let _ = store.add_log(&card.id, &text, "Hugh (board)");
            drop(store);
            let _ = a.emit("session-changed", card.id.clone());
            changed(&a);
        }
    });
    let _ = app.emit("resume-offer", ());
    Ok(n)
}
