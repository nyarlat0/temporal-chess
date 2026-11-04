-- Old rooms are pinned by their first protocol-v3 reconnect. New rooms always
-- store the creator's engine revision. Connected clients must match it; an empty
-- room may adopt a new build when an authorized player reconnects.
ALTER TABLE games ADD COLUMN engine_revision VARCHAR(64);

CREATE TABLE command_proposals (
    game_id VARCHAR(12) PRIMARY KEY REFERENCES games(game_id) ON DELETE CASCADE,
    proposal_id VARCHAR(32) NOT NULL UNIQUE,
    revision BIGINT NOT NULL CHECK (revision >= 0),
    side VARCHAR(5) NOT NULL CHECK (side IN ('white', 'black')),
    command VARCHAR(192) NOT NULL,
    white_accepted BOOLEAN NOT NULL DEFAULT FALSE,
    black_accepted BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
