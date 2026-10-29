use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_GAME_ID_LEN: usize = 12;
pub const MAX_COMMAND_LEN: usize = 192;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    White,
    Black,
}

impl Side {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidePreference {
    Random,
    White,
    Black,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    CreateGame {
        protocol: u16,
        side: SidePreference,
    },
    JoinGame {
        protocol: u16,
        game_id: String,
        player_token: Option<String>,
    },
    PlayCommand {
        revision: u64,
        command: String,
    },
    Resync {
        revision: u64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncReason {
    Requested,
    RevisionMismatch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    GameReady {
        created: bool,
        game_id: String,
        player_token: String,
        side: Side,
        revision: u64,
        history: Vec<String>,
        opponent_connected: bool,
    },
    CommandAccepted {
        revision: u64,
        command: String,
    },
    OpponentConnection {
        connected: bool,
    },
    Sync {
        revision: u64,
        history: Vec<String>,
        reason: SyncReason,
    },
    Error {
        code: String,
        message: String,
    },
}

/// Bound replay cost and keep snapshots below the record importer limit.
pub const MAX_COMMANDS: usize = 10_000;
