-- Independent room options. Legacy causal_gaps is kept only to identify rooms
-- which cannot be replayed after removing that ruleset; their journals are retained.
ALTER TABLE games ADD COLUMN chain_info_activation BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE games ADD COLUMN global_reaction_reset BOOLEAN NOT NULL DEFAULT FALSE;
