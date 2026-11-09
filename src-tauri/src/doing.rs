//! What a card's session is running (Hugh, card #150): its subagents, by name and since when,
//! so the board's status chip can say "Reviewer running" or "Tester running" instead of
//! "Building". Fed by the same hooks as `testing.rs`, by the same rules:
//!
//! - An `Agent`/`Task` call that runs in the foreground counts from its `PreToolUse` to its
//!   `PostToolUse`.
//! - A call that goes to the background answers its `PostToolUse` with the subagent's
//!   `agentId`; that subagent counts until its `SubagentStop`, after the turn too.
//! - The turn's end (`Stop`, or an interrupt the transcript recorded) ends the session's own
//!   foreground calls, not those of a background subagent still running. `SessionEnd` clears all.
//!
//! Skills aren't here: a review skill (`/code-review`) is `testing.rs`'s "Reviewing".

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

/// A background subagent whose `SubagentStop` never came (the session was killed mid-run)
/// stops counting after this long, as in `testing.rs`.
const AGENT_CAP_SECS: i64 = 3 * 60 * 60;

/// How many finished subagent ids are remembered, in case a `SubagentStop` comes before the
/// `PostToolUse` that names the subagent.
const STOPPED_KEPT: usize = 64;

/// The longest description kept, in characters.
const DESC_MAX: usize = 120;

/// The `status` of a call's response that says its subagent has already finished.
const FINISHED: [&str; 5] = ["completed", "failed", "error", "killed", "cancelled"];

/// One subagent running.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sub {
    /// Its `subagent_type` ("card-reviewer", "Explore"); "general-purpose" when the call named none.
    pub agent: String,
    /// The call's description ("Review card #150's diff"), cut to `DESC_MAX` characters.
    pub description: String,
    /// When it started (its `PreToolUse`, or its `PostToolUse` when that came first); RFC 3339
    /// in the JSON.
    #[serde(serialize_with = "rfc3339")]
    pub since: DateTime<Utc>,
    /// It runs in the background: it may outlive the turn that started it.
    pub background: bool,
}

/// One session's subagents in flight.
#[derive(Default)]
pub struct Running {
    /// Foreground calls, by `tool_use_id`, with the subagent that made each (None for the
    /// session itself).
    calls: HashMap<String, (Option<String>, Sub)>,
    /// Background subagents, by agent id.
    agents: HashMap<String, Sub>,
    /// Subagents that have stopped, newest last.
    stopped: VecDeque<String>,
}

fn rfc3339<S: serde::Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn is_agent_call(tool: &str) -> bool {
    matches!(tool, "Agent" | "Task")
}

/// The subagent a call's input describes, starting `now`.
fn sub_of(input: &Value, now: DateTime<Utc>) -> Sub {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let full = s("description").unwrap_or("");
    let mut description: String = full.chars().take(DESC_MAX).collect();
    if full.chars().count() > DESC_MAX {
        description.push('…');
    }
    Sub { agent: s("subagent_type").unwrap_or("general-purpose").to_string(), description, since: now, background: false }
}

impl Running {
    /// Takes in one hook. Returns whether what runs changed.
    pub fn hook(&mut self, event: &str, body: &Value, now: DateTime<Utc>) -> bool {
        let s = |k: &str| body.get(k).and_then(Value::as_str);
        let tool = s("tool_name").unwrap_or("");
        // Only these hooks can change it: the rest return before any work, under the sessions' lock.
        let ours = match event {
            "PreToolUse" | "PostToolUse" => is_agent_call(tool),
            "SubagentStop" | "Stop" | "StopFailure" | "SessionEnd" => true,
            _ => false,
        };
        if !ours {
            return false;
        }
        let before = self.all();
        let input = body.get("tool_input").unwrap_or(&Value::Null);
        let by = s("agent_id").map(str::to_string);
        match event {
            "PreToolUse" if is_agent_call(tool) => {
                if let Some(id) = s("tool_use_id") {
                    self.calls.insert(id.to_string(), (by, sub_of(input, now)));
                }
            }
            "PostToolUse" if is_agent_call(tool) => {
                let started = s("tool_use_id").and_then(|id| self.calls.remove(id)).map(|(_, sub)| sub);
                let resp = body.get("tool_response").unwrap_or(&Value::Null);
                let status = resp.get("status").and_then(Value::as_str).unwrap_or("");
                if let Some(agent) = resp.get("agentId").and_then(Value::as_str) {
                    if !self.stopped.iter().any(|s| s == agent) && !FINISHED.contains(&status) {
                        let mut sub = started.unwrap_or_else(|| sub_of(input, now));
                        sub.background = true;
                        self.agents.insert(agent.to_string(), sub);
                    }
                }
            }
            "SubagentStop" => {
                if let Some(agent) = by {
                    self.agents.remove(&agent);
                    // Whatever it called in the foreground ended with it.
                    self.calls.retain(|_, (b, _)| b.as_deref() != Some(agent.as_str()));
                    self.stopped.push_back(agent);
                    if self.stopped.len() > STOPPED_KEPT {
                        self.stopped.pop_front();
                    }
                }
            }
            "Stop" | "StopFailure" => {
                self.turn_ended();
            }
            "SessionEnd" => *self = Running::default(),
            _ => {}
        }
        self.agents.retain(|_, sub| now - sub.since < Duration::seconds(AGENT_CAP_SECS));
        before != self.all()
    }

    /// The turn ended: the session's own foreground calls ended with it, but not those of a
    /// background subagent still running. Returns whether anything stopped counting.
    pub fn turn_ended(&mut self) -> bool {
        let n = self.calls.len();
        self.calls.retain(|_, (b, _)| b.is_some());
        n != self.calls.len()
    }

    /// What runs now, oldest first: the foreground calls and the background subagents, less
    /// any background one past the cap whose hook to drop it hasn't come.
    pub fn list(&self, now: DateTime<Utc>) -> Vec<Sub> {
        let cap = Duration::seconds(AGENT_CAP_SECS);
        self.all().into_iter().filter(|s| !s.background || now - s.since < cap).collect()
    }

    /// Everything it holds, oldest first, with no cap: what `hook` compares.
    fn all(&self) -> Vec<Sub> {
        let mut v: Vec<Sub> = self.calls.values().map(|(_, s)| s.clone()).chain(self.agents.values().cloned()).collect();
        v.sort_by(|a, b| a.since.cmp(&b.since).then_with(|| a.agent.cmp(&b.agent)).then_with(|| a.description.cmp(&b.description)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-09T10:00:00Z").unwrap().with_timezone(&Utc)
    }
    fn pre(id: &str, input: Value) -> Value {
        json!({ "tool_name": "Agent", "tool_use_id": id, "tool_input": input })
    }
    fn post(id: &str, input: Value, resp: Value) -> Value {
        json!({ "tool_name": "Agent", "tool_use_id": id, "tool_input": input, "tool_response": resp })
    }

    #[test]
    fn a_foreground_subagent_runs_until_its_call_returns() {
        let mut r = Running::default();
        let now = t0();
        let input = json!({ "description": "Review card #150's diff", "subagent_type": "card-reviewer", "prompt": "x" });
        assert!(r.hook("PreToolUse", &pre("u1", input.clone()), now));
        let l = r.list(now);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0], Sub { agent: "card-reviewer".into(), description: "Review card #150's diff".into(), since: now, background: false });
        // Its own SubagentStop comes first; the call's return names it as finished.
        assert!(!r.hook("SubagentStop", &json!({ "agent_id": "a1", "agent_type": "card-reviewer" }), now), "the call still counts until it returns");
        assert!(r.hook("PostToolUse", &post("u1", input, json!({ "status": "completed", "agentId": "a1" })), now + Duration::seconds(90)));
        assert!(r.list(now).is_empty());
    }

    #[test]
    fn a_background_subagent_outlives_the_turn_until_it_stops() {
        let mut r = Running::default();
        let now = t0();
        let input = json!({ "description": "Test card #150 in the test app", "subagent_type": "card-tester", "run_in_background": true });
        r.hook("PreToolUse", &pre("u2", input.clone()), now);
        let later = now + Duration::seconds(5);
        r.hook("PostToolUse", &post("u2", input, json!({ "isAsync": true, "status": "async_launched", "agentId": "a2" })), later);
        assert!(!r.hook("Stop", &Value::Null, later), "the turn's end doesn't stop it");
        let l = r.list(later);
        assert_eq!(l.len(), 1);
        assert!(l[0].background);
        assert_eq!(l[0].since, now, "it counts from its PreToolUse");
        assert_eq!(l[0].agent, "card-tester");
        assert!(r.hook("SubagentStop", &json!({ "agent_id": "a2" }), later));
        assert!(r.list(later).is_empty());
    }

    #[test]
    fn a_stop_before_the_launch_report_is_not_counted_again() {
        let mut r = Running::default();
        let now = t0();
        let input = json!({ "description": "Quick look", "subagent_type": "Explore" });
        r.hook("PreToolUse", &pre("u3", input.clone()), now);
        r.hook("SubagentStop", &json!({ "agent_id": "a3" }), now);
        r.hook("PostToolUse", &post("u3", input, json!({ "status": "async_launched", "agentId": "a3" })), now);
        assert!(r.list(now).is_empty());
    }

    #[test]
    fn the_turn_end_clears_the_sessions_own_calls_and_session_end_clears_all() {
        let mut r = Running::default();
        let now = t0();
        r.hook("PreToolUse", &pre("u4", json!({ "description": "Find the hook code" })), now);
        assert_eq!(r.list(now)[0].agent, "general-purpose", "a call that names no type");
        // A background subagent's own foreground call ends only with that subagent.
        let mut inner = pre("u5", json!({ "description": "Inner look", "subagent_type": "Explore" }));
        inner["agent_id"] = "a9".into();
        r.hook("PreToolUse", &inner, now);
        assert!(r.hook("Stop", &Value::Null, now));
        assert_eq!(r.list(now).len(), 1);
        assert!(r.hook("SubagentStop", &json!({ "agent_id": "a9" }), now));
        assert!(r.list(now).is_empty());
        r.hook("PostToolUse", &post("u6", json!({ "subagent_type": "card-critic" }), json!({ "agentId": "a6" })), now);
        assert_eq!(r.list(now).len(), 1, "a launch report alone counts too");
        assert!(r.hook("SessionEnd", &Value::Null, now));
        assert!(r.list(now).is_empty());
    }

    #[test]
    fn other_tools_dont_count_and_a_lost_stop_is_capped() {
        let mut r = Running::default();
        let now = t0();
        assert!(!r.hook("PreToolUse", &json!({ "tool_name": "Skill", "tool_use_id": "s1", "tool_input": { "skill": "code-review" } }), now));
        assert!(!r.hook("PreToolUse", &json!({ "tool_name": "Bash", "tool_use_id": "b1", "tool_input": { "description": "Run tests" } }), now));
        r.hook("PostToolUse", &post("u7", json!({ "subagent_type": "card-tester" }), json!({ "agentId": "a7" })), now);
        assert_eq!(r.list(now).len(), 1);
        assert!(!r.hook("Notification", &Value::Null, now + Duration::seconds(AGENT_CAP_SECS)), "a hook that can't change it returns at once");
        assert!(r.list(now + Duration::seconds(AGENT_CAP_SECS)).is_empty(), "the list leaves it out past the cap");
        assert!(r.hook("SubagentStop", &json!({ "agent_id": "other" }), now + Duration::seconds(AGENT_CAP_SECS)), "the next hook that can drops it");
    }

    #[test]
    fn it_serialises_for_the_board() {
        let sub = Sub { agent: "card-tester".into(), description: "Test it".into(), since: t0(), background: true };
        assert_eq!(serde_json::to_value(&sub).unwrap(), json!({ "agent": "card-tester", "description": "Test it", "since": "2026-11-09T10:00:00Z", "background": true }));
    }

    #[test]
    fn a_long_description_is_cut() {
        let mut r = Running::default();
        let now = t0();
        r.hook("PreToolUse", &pre("u8", json!({ "description": "x".repeat(300) })), now);
        assert_eq!(r.list(now)[0].description.chars().count(), DESC_MAX + 1, "cut, with an ellipsis");
        let mut r = Running::default();
        r.hook("PreToolUse", &pre("u9", json!({ "description": "y".repeat(DESC_MAX) })), now);
        assert_eq!(r.list(now)[0].description, "y".repeat(DESC_MAX), "exactly the most is kept whole");
    }
}
