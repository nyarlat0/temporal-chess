CREATE TABLE games (
    game_id VARCHAR(12) PRIMARY KEY,
    white_token_hash BYTEA,
    black_token_hash BYTEA,
    revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (white_token_hash IS NOT NULL OR black_token_hash IS NOT NULL)
);

CREATE TABLE game_commands (
    game_id VARCHAR(12) NOT NULL REFERENCES games(game_id) ON DELETE CASCADE,
    revision BIGINT NOT NULL CHECK (revision > 0),
    command VARCHAR(192) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (game_id, revision)
);
