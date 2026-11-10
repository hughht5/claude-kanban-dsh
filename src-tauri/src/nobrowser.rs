//! No computer use and no browser when this app's own cards test (Hugh, card #172): "for testing
//! i want no computer use or browser use". A card session for one of `REPOS` gets a deny rule
//! for each of `SERVERS` in its own settings (`sessions::settings_json`), and no headless
//! chrome-devtools server in its `mcp.json` (`sessions::mcp_json`). A deny rule naming a whole
//! server (`mcp__<server>`) takes every tool of it away from the session and from every subagent
//! it runs, the app's own included, even under `bypassPermissions` (card #172's live trial,
//! Claude Code 2.1.246). The card's own test app stays open to it: `tools/cdp.mjs` drives the
//! test app's WebView over its DevTools port and takes its shots, and is no browser.
//!
//! Other repos keep their browser until Hugh says otherwise: widening the rule is one
//! line in `REPOS`.

/// The repos whose card sessions may not use computer use or a browser: this app, under its
/// name and its old one.
pub const REPOS: [&str; 2] = ["hughht5/claude-kanban-dsh", "hughht5/kanban-app"];

/// The MCP servers that drive the desktop or a browser: computer use (the desktop app's, and a
/// cloud session's linked computer), Claude in Chrome, the desktop app's built-in browser and
/// preview pane, and the Chrome DevTools server.
pub const SERVERS: [&str; 6] = ["computer-use", "remote-devices", "claude-in-chrome", "Claude_Browser", "Claude_Preview", "chrome-devtools"];

/// Whether a card of `repo` tests without computer use or a browser.
pub fn applies(repo: Option<&str>) -> bool {
    repo.map(str::trim).is_some_and(|r| REPOS.iter().any(|x| x.eq_ignore_ascii_case(r)))
}

/// The session's deny rules: one per server, naming the whole server.
pub fn deny_rules() -> Vec<String> {
    SERVERS.iter().map(|s| format!("mcp__{s}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_apps_cards() {
        assert!(applies(Some("hughht5/claude-kanban-dsh")));
        assert!(applies(Some("HughHT5/Kanban-App ")));
        assert!(!applies(Some("someone/web-app")), "other repos keep their browser until Hugh says");
        assert!(!applies(Some("")));
        assert!(!applies(None));
    }

    #[test]
    fn denies_every_computer_and_browser_server_whole() {
        let d = deny_rules();
        for s in ["mcp__computer-use", "mcp__remote-devices", "mcp__claude-in-chrome", "mcp__Claude_Browser", "mcp__Claude_Preview", "mcp__chrome-devtools"] {
            assert!(d.iter().any(|x| x == s), "denies {s}");
        }
        // A rule with a tool or specifier would deny only part of a server.
        assert!(d.iter().all(|x| x.matches("__").count() == 1), "{d:?}");
        assert!(!d.iter().any(|x| x.contains("kanban")), "the board's own tools stay");
    }

    #[test]
    fn the_tester_and_reproducer_say_so() {
        for a in [&crate::tester::AGENT, &crate::repro::AGENT] {
            for want in ["hughht5/claude-kanban-dsh", "no computer use", "no browser", "chrome-devtools", "tools/cdp.mjs", "card #172"] {
                assert!(a.prompt.contains(want), "{} says \"{want}\"", a.name);
            }
        }
    }
}
