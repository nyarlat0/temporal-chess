use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 6;
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
        #[serde(default)]
        time_war_separation: Option<usize>,
        protocol: u16,
        #[serde(default)]
        engine_revision: String,
        #[serde(default)]
        chain_info_activation: bool,
        #[serde(default)]
        global_reaction_reset: bool,
        #[serde(default)]
        wait_before_move: bool,
        side: SidePreference,
    },
    JoinGame {
        protocol: u16,
        #[serde(default)]
        engine_revision: String,
        game_id: String,
        player_token: Option<String>,
    },
    PlayCommand {
        revision: u64,
        command: String,
    },
    CommandVote {
        proposal_id: String,
        revision: u64,
        accepted: bool,
        reason: Option<String>,
    },
    Resync {
        revision: u64,
    },
}

/// An opaque command awaiting independent validation by both player clients.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandProposal {
    pub id: String,
    pub revision: u64,
    pub side: Side,
    pub command: String,
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
        time_war_separation: Option<usize>,
        engine_revision: String,
        chain_info_activation: bool,
        global_reaction_reset: bool,
        wait_before_move: bool,
        created: bool,
        game_id: String,
        player_token: String,
        side: Side,
        revision: u64,
        history: Vec<String>,
        opponent_connected: bool,
        pending: Option<CommandProposal>,
    },
    CommandProposed {
        proposal: CommandProposal,
    },
    CommandRejected {
        proposal_id: String,
        side: Side,
        reason: String,
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
        pending: Option<CommandProposal>,
    },
    Error {
        code: String,
        message: String,
    },
}

/// Bound replay cost and keep snapshots below the record importer limit.
pub const MAX_COMMANDS: usize = 10_000;
pub const MAX_REJECTION_LEN: usize = 512;
