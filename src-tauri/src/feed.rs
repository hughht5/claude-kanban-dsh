//! The agents' feed (card #120): what the app's agents said and decided, newest first, for the
//! page. Read only; the work is in the store (`crates/store/src/feed.rs`).

use crate::{Board, CmdError, CmdResult};
use kanban_store::{FeedPage, FeedQuery};
use tauri::State;

/// `invoke("agents_feed", {query: {cards?, agents?, kinds?, since?, before?, limit?}})`.
#[tauri::command]
pub fn agents_feed(board: State<Board>, query: Option<FeedQuery>) -> CmdResult<FeedPage> {
    let store = board.0.lock().map_err(CmdError::other)?;
    store.agents_feed(&query.unwrap_or_default()).map_err(CmdError::other)
}

#[cfg(test)]
mod tests {
    use kanban_store::{agent_of_by, agent_of_field, VERDICT_FIELDS};

    /// The app's own writers still map to their agents: a renamed `BY` would drop them from the feed.
    #[test]
    fn the_apps_writers_map_to_their_agents() {
        for (by, agent) in [
            (crate::tester::BY, "tester"),
            (crate::reviewer::BY, "reviewer"),
            (crate::critic::CHECK.by, "critic"),
            (crate::repro::BY, "reproducer"),
            (crate::merger::BY, "merger"),
            (crate::router::BY, "router"),
            (crate::monitor::BY, "monitor"),
        ] {
            assert_eq!(agent_of_by(by), Some(agent), "{by}");
        }
        assert_eq!(agent_of_by("the app"), None);
        assert_eq!(agent_of_by(kanban_store::AUTO_BY), None);
        assert!(VERDICT_FIELDS.iter().all(|f| agent_of_field(f).is_some()));
        assert_eq!(crate::monitor::STUCK_KEY, "stuck");
        assert_eq!(kanban_store::ROUTE_KEY, "route");
    }
}
