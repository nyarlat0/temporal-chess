-- Undo shares the pending slot with commands: consent and moves cannot race.
ALTER TABLE command_proposals ADD COLUMN undo_keep_commands INTEGER
    CHECK (undo_keep_commands >= 0 AND undo_keep_commands < 10000);
