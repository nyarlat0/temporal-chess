-- Network revision increases on both moves and undo; track journal length
-- separately so proposing a command still reads only the locked game row.
ALTER TABLE games ADD COLUMN command_count INTEGER NOT NULL DEFAULT 0
    CHECK (command_count >= 0 AND command_count <= 10000);
UPDATE games SET command_count = (
    SELECT count(*) FROM game_commands WHERE game_commands.game_id = games.game_id
);
