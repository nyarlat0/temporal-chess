-- Previously created rooms used preserved futures. New rooms explicitly choose
-- their mode; the column default applies only to old rows during this migration.
ALTER TABLE games ADD COLUMN causal_gaps BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE games ALTER COLUMN causal_gaps SET DEFAULT FALSE;
