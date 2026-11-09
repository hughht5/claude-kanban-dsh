-- Migration 17 (card #162, 9 Nov 2026): a card's subagents hold only Hugh's overrides. Before it,
-- every card made since card #151 had all six stamped from the defaults of the day, so a later
-- change of a default in Settings never reached it. The work is in Rust, after this file
-- (subagents::unstamp_defaults, run by Store::migrate in the same transaction): on each card not
-- archived, a stamped value equal to the board's default now goes, so it follows Settings; one
-- that differs stays as Hugh's override. Each change is an ordinary card write with its revision.
-- Nothing is removed from any table. Safe to run twice. An older build opens a board at this
-- version as it is: a card with no override reads its subagents from Settings, as before.
SELECT 1;
