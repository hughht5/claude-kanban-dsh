//! "Testing" on a card (Hugh, card #86): while its session runs a review or a test, the card
//! reads "Testing" instead of "Building". The signal is the session's own hooks:
//!
//! - `PreToolUse`/`PostToolUse` for a `Skill` call (a review or test skill, such as
//!   `/code-review`) or an `Agent`/`Task` call whose description says it tests, reviews or
//!   verifies. A call that runs to the end in the foreground counts from its `PreToolUse` to its
//!   `PostToolUse`.
//! - A call that goes to the background (or forks) answers its `PostToolUse` with the
//!   subagent's `agentId`; that subagent then counts until its `SubagentStop`.
//! - A skill that runs inline returns from its `Skill` call as soon as its instructions load and
//!   then runs in the turn itself, as does a review or test slash command typed as the prompt
//!   (`UserPromptSubmit`): those count until the turn's `Stop`.

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// The tools that start a subagent or a skill.
pub const TOOLS: &str = "Agent|Task|Skill";

/// A background subagent whose `SubagentStop` never came (the session was killed mid-run)
/// stops counting after this long.
const AGENT_CAP: Duration = Duration::from_secs(3 * 60 * 60);

/// How many finished subagent ids are remembered, in case a `SubagentStop` comes before the
/// `PostToolUse` that names the subagent.
const STOPPED_KEPT: usize = 64;

/// A description that starts with one of these is building something, even when it names a
/// test: "Build the testing chip", "Write tests for the store".
const BUILD_VERBS: [&str; 12] = ["build", "add", "write", "implement", "create", "fix", "make", "refactor", "update", "change", "port", "wire"];

fn words(s: &str) -> Vec<String> {
    s.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

fn testing_word(w: &str) -> bool {
    w.starts_with("test") || w.starts_with("review") || w.starts_with("verif") || w == "qa"
}

/// Whether `s` says it tests: a word starting with "test", "review" or "verif", or "qa"
/// ("Run the unit tests", "code-review", "Verify the build"), and not a build ("Build the
/// testing chip").
fn says_testing(s: &str) -> bool {
    let w = words(s);
    !w.first().is_some_and(|f| BUILD_VERBS.contains(&f.as_str())) && w.iter().any(|w| testing_word(w))
}

/// Whether a call of `tool` with `input` is a review or a test: a skill by its name, a
/// subagent by its description or its type. A subagent's prompt doesn't count, because a
/// build's prompt names its tests too.
pub fn is_testing_call(tool: &str, input: &Value) -> bool {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    match tool {
        "Skill" => words(s("skill")).iter().any(|w| testing_word(w)),
        "Agent" | "Task" => says_testing(s("description")) || words(s("subagent_type")).iter().any(|w| testing_word(w)),
        _ => false,
    }
}

/// Whether a prompt is a review or test slash command: `/code-review high`, `/security-review`.
fn testing_command(prompt: &str) -> bool {
    let p = prompt.trim_start();
    p.strip_prefix('/').and_then(|r| r.split_whitespace().next()).is_some_and(|c| words(c).iter().any(|w| testing_word(w)))
}

/// One session's reviews and tests in flight.
#[derive(Default)]
pub struct Testing {
    /// Foreground calls, by `tool_use_id`, with the subagent that made each (None for the
    /// session itself).
    calls: HashMap<String, Option<String>>,
    /// Background subagents, by agent id, with when they started counting.
    agents: HashMap<String, Instant>,
    /// Subagents that have stopped, newest last.
    stopped: VecDeque<String>,
    /// The turn itself is a review or a test (an inline skill, or a slash command).
    turn: bool,
}

impl Testing {
    /// Takes in one hook. Returns whether the session is testing afterwards.
    pub fn hook(&mut self, event: &str, body: &Value, now: Instant) -> bool {
        let s = |k: &str| body.get(k).and_then(Value::as_str);
        let tool = s("tool_name").unwrap_or("");
        let input = body.get("tool_input").unwrap_or(&Value::Null);
        let by = s("agent_id").map(str::to_string);
        match event {
            "UserPromptSubmit" if testing_command(s("prompt").unwrap_or("")) => self.turn = true,
            "PreToolUse" if is_testing_call(tool, input) => {
                if let Some(id) = s("tool_use_id") {
                    self.calls.insert(id.to_string(), by);
                }
            }
            "PostToolUse" => {
                if let Some(id) = s("tool_use_id") {
                    self.calls.remove(id);
                }
                if is_testing_call(tool, input) {
                    match body.pointer("/tool_response/agentId").and_then(Value::as_str) {
                        Some(agent) if !self.stopped.iter().any(|s| s == agent) => {
                            self.agents.insert(agent.to_string(), now);
                        }
                        Some(_) => {}
                        // An inline skill: it runs in the turn from here on.
                        None if tool == "Skill" && by.is_none() => self.turn = true,
                        None => {}
                    }
                }
            }
            "SubagentStop" => {
                if let Some(agent) = by {
                    self.agents.remove(&agent);
                    self.calls.retain(|_, b| b.as_deref() != Some(agent.as_str()));
                    self.stopped.push_back(agent);
                    if self.stopped.len() > STOPPED_KEPT {
                        self.stopped.pop_front();
                    }
                }
            }
            // The turn's end: the session's own foreground calls ended with it, but not those
            // of a background subagent still running.
            "Stop" | "StopFailure" => self.turn_ended(),
            "SessionEnd" => *self = Testing::default(),
            _ => {}
        }
        self.agents.retain(|_, t| now.duration_since(*t) < AGENT_CAP);
        self.active(now)
    }

    /// The turn ended (its `Stop`, or an interrupt the transcript recorded).
    pub fn turn_ended(&mut self) {
        self.calls.retain(|_, b| b.is_some());
        self.turn = false;
    }

    pub fn active(&self, now: Instant) -> bool {
        self.turn || !self.calls.is_empty() || self.agents.values().any(|t| now.duration_since(*t) < AGENT_CAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pre(tool: &str, id: &str, input: Value) -> Value {
        json!({ "tool_name": tool, "tool_use_id": id, "tool_input": input })
    }

    #[test]
    fn reviews_and_tests_count_and_builds_dont() {
        assert!(is_testing_call("Skill", &json!({ "skill": "code-review", "args": "low" })));
        assert!(is_testing_call("Skill", &json!({ "skill": "security-review" })));
        assert!(is_testing_call("Agent", &json!({ "description": "Run the unit tests", "prompt": "x" })));
        assert!(is_testing_call("Agent", &json!({ "description": "Verify in the test app" })));
        assert!(is_testing_call("Task", &json!({ "description": "QA pass" })));
        assert!(is_testing_call("Agent", &json!({ "description": "x", "subagent_type": "reviewer" })));
        // A build whose prompt or subject mentions tests is still a build.
        assert!(!is_testing_call("Agent", &json!({ "description": "Build the store migration", "prompt": "add tests" })));
        assert!(!is_testing_call("Agent", &json!({ "description": "Build the testing chip" })));
        assert!(!is_testing_call("Agent", &json!({ "description": "Write tests for the store" })));
        assert!(!is_testing_call("Agent", &json!({ "description": "Find the latest attestation code" })));
        assert!(!is_testing_call("Skill", &json!({ "skill": "kanban" })));
        assert!(!is_testing_call("Bash", &json!({ "description": "Run tests" })));
    }

    #[test]
    fn a_foreground_review_counts_until_it_returns() {
        let mut t = Testing::default();
        let now = Instant::now();
        assert!(t.hook("PreToolUse", &pre("Skill", "u1", json!({ "skill": "code-review" })), now));
        assert!(t.hook("SubagentStop", &json!({ "agent_id": "a1" }), now), "its fork stopping doesn't end the call");
        let post = json!({ "tool_name": "Skill", "tool_use_id": "u1", "tool_input": { "skill": "code-review" },
            "tool_response": { "status": "forked", "agentId": "a1", "result": "(none)" } });
        assert!(!t.hook("PostToolUse", &post, now), "a fork that already stopped isn't counted again");
    }

    #[test]
    fn an_inline_skill_or_a_typed_command_counts_until_the_turn_ends() {
        let now = Instant::now();
        let mut t = Testing::default();
        t.hook("PreToolUse", &pre("Skill", "u7", json!({ "skill": "code-review" })), now);
        let post = json!({ "tool_name": "Skill", "tool_use_id": "u7", "tool_input": { "skill": "code-review" }, "tool_response": { "success": true } });
        assert!(t.hook("PostToolUse", &post, now), "the review runs in the turn after the call returns");
        assert!(!t.hook("Stop", &Value::Null, now));
        assert!(t.hook("UserPromptSubmit", &json!({ "prompt": "/code-review high" }), now));
        assert!(!t.hook("Stop", &Value::Null, now));
        assert!(!t.hook("UserPromptSubmit", &json!({ "prompt": "Please review the diff" }), now), "only a slash command");
        assert!(!t.hook("UserPromptSubmit", &json!({ "prompt": "/kanban 12" }), now));
    }

    #[test]
    fn a_background_test_counts_until_its_subagent_stops() {
        let mut t = Testing::default();
        let now = Instant::now();
        let input = json!({ "description": "Run the unit tests", "subagent_type": "general-purpose" });
        assert!(t.hook("PreToolUse", &pre("Agent", "u2", input.clone()), now));
        let post = json!({ "tool_name": "Agent", "tool_use_id": "u2", "tool_input": input,
            "tool_response": { "isAsync": true, "status": "async_launched", "agentId": "a2" } });
        assert!(t.hook("PostToolUse", &post, now));
        assert!(t.hook("Stop", &Value::Null, now), "it outlives the turn that launched it");
        assert!(!t.hook("SubagentStop", &json!({ "agent_id": "a2" }), now));
        // Capped, in case its stop never came.
        let mut t = Testing::default();
        t.hook("PostToolUse", &post, now);
        assert!(!t.hook("Notification", &Value::Null, now + AGENT_CAP));
    }

    #[test]
    fn a_test_inside_a_background_subagent_outlives_the_turn() {
        let mut t = Testing::default();
        let now = Instant::now();
        let mut b = pre("Agent", "u8", json!({ "description": "Run the unit tests" }));
        b["agent_id"] = "a8".into();
        assert!(t.hook("PreToolUse", &b, now));
        assert!(t.hook("Stop", &Value::Null, now));
        assert!(!t.hook("SubagentStop", &json!({ "agent_id": "a8" }), now), "its subagent ending ends it");
    }

    #[test]
    fn a_build_subagent_doesnt_count() {
        let mut t = Testing::default();
        let now = Instant::now();
        let input = json!({ "description": "Build the panel change" });
        assert!(!t.hook("PreToolUse", &pre("Agent", "u3", input.clone()), now));
        let post = json!({ "tool_name": "Agent", "tool_use_id": "u3", "tool_input": input, "tool_response": { "agentId": "a3" } });
        assert!(!t.hook("PostToolUse", &post, now));
    }

    #[test]
    fn an_interrupted_turn_or_the_session_ending_clears_it() {
        let now = Instant::now();
        let mut t = Testing::default();
        t.hook("PreToolUse", &pre("Skill", "u4", json!({ "skill": "code-review" })), now);
        t.turn_ended();
        assert!(!t.active(now));
        let post = json!({ "tool_name": "Agent", "tool_use_id": "u6", "tool_input": { "description": "Review the diff" }, "tool_response": { "agentId": "a6" } });
        assert!(t.hook("PostToolUse", &post, now));
        assert!(!t.hook("SessionEnd", &Value::Null, now));
    }
}
