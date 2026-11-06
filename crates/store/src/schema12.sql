-- Migration 12 (card #109, 6 Nov 2026): cards that wait on others. A card's waits are in its
-- fields (`blockedBy: [12, 40]`, and `waitedOn`, which the app writes), so no table changes.
-- This only adds a small partial index, so the app's 2-second poll finds the waiting cards (or
-- that there are none) without reading every card. It changes no row. It is safe to run twice,
-- and an older build simply keeps the index up to date as it writes.
CREATE INDEX IF NOT EXISTS cards_waiting ON cards(archived)
  WHERE json_extract(fields,'$.blockedBy') IS NOT NULL OR json_extract(fields,'$.waitedOn') IS NOT NULL;
