-- ASPC C6.4: a meeting marked Private is left out of all-meetings Ask,
-- bulk exports and sharing unless the user includes it for that action.
-- Separate from `locked` (device authentication to reveal a note).
ALTER TABLE sessions ADD COLUMN private INTEGER NOT NULL DEFAULT 0;
