-- Existing games used free standalone waits. New games send their choice explicitly.
ALTER TABLE games ADD COLUMN wait_before_move BOOLEAN NOT NULL DEFAULT TRUE;
