-- Migration 8 (Hugh, 4 Nov 2026): stacked cards (design/stacked-cards/SPEC.md). A stack's
-- membership lives in the cards' own `fields` JSON (`stack` on the lead, `stackOf` on a child,
-- `passed`, `archivedWith`), so no card row changes here: every existing card stays a loose card.
-- This only records when a stack moves to Human test: when every child is ready ("all"), unless
-- Hugh picks "first" (as soon as one is). A board that already has the setting keeps it.
INSERT OR IGNORE INTO settings (key, value) VALUES ('stacks.human_test_when', 'all');
