use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use temporal_chess_protocol::{
    CommandProposal, MAX_COMMAND_LEN, MAX_COMMANDS, MAX_GAME_ID_LEN, MAX_REJECTION_LEN, Side,
    SidePreference, SyncReason, UndoProposal,
};
use thiserror::Error;
use uuid::Uuid;

const GAME_ID_LEN: usize = 10;
const GAME_ID_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const CREATE_RETRIES: usize = 8;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct Repository {
    pool: PgPool,
}

#[derive(Clone, Debug)]
pub(crate) struct AuthenticatedPlayer {
    pub(crate) game_id: String,
    pub(crate) side: Side,
    pub(crate) player_token: String,
}

#[derive(Debug)]
pub(crate) struct ReadyGame {
    pub(crate) engine_revision: String,
    pub(crate) pending: Option<CommandProposal>,
    pub(crate) pending_undo: Option<UndoProposal>,
    pub(crate) chain_info_activation: bool,
    pub(crate) global_reaction_reset: bool,
    pub(crate) wait_before_move: bool,
    pub(crate) time_war_separation: Option<usize>,
    pub(crate) auth: AuthenticatedPlayer,
    pub(crate) revision: u64,
    pub(crate) history: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum CommandResult {
    Proposed(CommandProposal),
    UndoProposed(UndoProposal),
    UndoDeclined {
        proposal_id: String,
    },
    UndoAccepted {
        proposal_id: String,
        revision: u64,
        history: Vec<String>,
    },
    Rejected {
        proposal_id: String,
        side: Side,
        reason: String,
    },
    Unchanged,
    Accepted {
        revision: u64,
        command: String,
    },
    Sync {
        revision: u64,
        history: Vec<String>,
        reason: SyncReason,
        pending: Option<CommandProposal>,
        pending_undo: Option<UndoProposal>,
    },
}

#[derive(Debug, Error)]
pub(crate) enum RepositoryError {
    #[error("{message}")]
    Client { code: &'static str, message: String },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("stored game {game_id} is inconsistent: {detail}")]
    Corrupt { game_id: String, detail: String },
}

#[derive(FromRow)]
struct RoomRow {
    engine_revision: Option<String>,
    causal_gaps: bool, // Legacy rooms with this removed option cannot be replayed.
    chain_info_activation: bool,
    global_reaction_reset: bool,
    wait_before_move: bool,
    time_war_separation: Option<i32>,
    game_id: String,
    white_token_hash: Option<Vec<u8>>,
    black_token_hash: Option<Vec<u8>>,
    revision: i64,
    command_count: i32,
}

impl Repository {
    pub(crate) const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn create_game(
        &self,
        preference: SidePreference,
        chain_info_activation: bool,
        global_reaction_reset: bool,
        wait_before_move: bool,
        engine_revision: &str,
        time_war_separation: Option<usize>,
    ) -> Result<ReadyGame, RepositoryError> {
        validate_engine_revision(engine_revision)?;
        if time_war_separation.is_some_and(|n| !(3..=255).contains(&n) || n % 2 == 0) {
            return Err(client_error(
                "invalid_starting_position",
                "Time War separation must be odd and between 3 and 255.",
            ));
        }
        let side = resolve_side(preference);
        let player_token = generate_token();
        let token_hash = hash_token(&player_token);

        for _ in 0..CREATE_RETRIES {
            let game_id = generate_game_id();
            let (white_hash, black_hash) = match side {
                Side::White => (Some(token_hash.clone()), None),
                Side::Black => (None, Some(token_hash.clone())),
            };
            let result = sqlx::query(
                "INSERT INTO games
                    (game_id, white_token_hash, black_token_hash, revision, chain_info_activation, global_reaction_reset, engine_revision, time_war_separation, wait_before_move)
                 VALUES ($1, $2, $3, 0, $4, $5, $6, $7, $8)
                 ON CONFLICT (game_id) DO NOTHING",
            )
            .bind(&game_id)
            .bind(white_hash)
            .bind(black_hash)
            .bind(chain_info_activation)
            .bind(global_reaction_reset)
            .bind(engine_revision)
            .bind(time_war_separation.map(|n| n as i32))
            .bind(wait_before_move)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() == 1 {
                return Ok(ReadyGame {
                    engine_revision: engine_revision.to_owned(),
                    pending: None,
                    pending_undo: None,
                    chain_info_activation,
                    global_reaction_reset,
                    wait_before_move,
                    time_war_separation,
                    auth: AuthenticatedPlayer {
                        game_id,
                        side,
                        player_token,
                    },
                    revision: 0,
                    history: Vec::new(),
                });
            }
        }

        Err(client_error(
            "game_id_exhausted",
            "Could not allocate a game id. Please try again.",
        ))
    }

    pub(crate) async fn join_game(
        &self,
        game_id: &str,
        player_token: Option<&str>,
        engine_revision: &str,
        allow_engine_change: bool,
    ) -> Result<ReadyGame, RepositoryError> {
        validate_engine_revision(engine_revision)?;
        if player_token
            .is_some_and(|token| token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(client_error(
                "invalid_player_token",
                "Некорректный токен игрока.",
            ));
        }
        let game_id = normalize_game_id(game_id)?;
        let mut transaction = self.pool.begin().await?;
        let mut room = sqlx::query_as::<_, RoomRow>(
            "SELECT game_id, white_token_hash, black_token_hash, revision, command_count, causal_gaps, chain_info_activation, global_reaction_reset, engine_revision, time_war_separation, wait_before_move
             FROM games WHERE game_id = $1 FOR UPDATE",
        )
        .bind(&game_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| client_error("game_not_found", "No game exists with that id."))?;

        if room.causal_gaps {
            return Err(client_error(
                "removed_room_rules",
                "This room uses the removed causal gaps mode. Create a new room.",
            ));
        }

        if !allow_engine_change
            && room
                .engine_revision
                .as_deref()
                .is_some_and(|revision| revision != engine_revision)
        {
            return Err(client_error(
                "engine_revision_mismatch",
                "The room uses a different engine revision.",
            ));
        }
        if room.engine_revision.as_deref() != Some(engine_revision) {
            sqlx::query("UPDATE games SET engine_revision = $2 WHERE game_id = $1")
                .bind(&game_id)
                .bind(engine_revision)
                .execute(&mut *transaction)
                .await?;
            // Approval under another engine revision must never carry over.
            sqlx::query("DELETE FROM command_proposals WHERE game_id = $1")
                .bind(&game_id)
                .execute(&mut *transaction)
                .await?;
        }

        let (side, player_token) = if let Some(token) = player_token {
            (authenticate(&room, token)?, token.to_owned())
        } else {
            let side = match (
                room.white_token_hash.is_none(),
                room.black_token_hash.is_none(),
            ) {
                (true, false) => Side::White,
                (false, true) => Side::Black,
                (true, true) => {
                    return Err(RepositoryError::Corrupt {
                        game_id,
                        detail: "neither player slot is claimed".to_owned(),
                    });
                }
                (false, false) => {
                    return Err(client_error(
                        "game_full",
                        "Both colors are already claimed. A saved player token is required.",
                    ));
                }
            };
            let token = generate_token();
            let token_hash = hash_token(&token);
            match side {
                Side::White => {
                    sqlx::query("UPDATE games SET white_token_hash = $2 WHERE game_id = $1")
                        .bind(&room.game_id)
                        .bind(&token_hash)
                        .execute(&mut *transaction)
                        .await?;
                    room.white_token_hash = Some(token_hash);
                }
                Side::Black => {
                    sqlx::query("UPDATE games SET black_token_hash = $2 WHERE game_id = $1")
                        .bind(&room.game_id)
                        .bind(&token_hash)
                        .execute(&mut *transaction)
                        .await?;
                    room.black_token_hash = Some(token_hash);
                }
            }
            (side, token)
        };

        let history = fetch_history(&mut transaction, &room.game_id).await?;
        verify_revision(&room, &history)?;
        let revision = revision_to_u64(room.revision, &room.game_id)?;
        let (pending, pending_undo) = pending_proposals(&mut transaction, &room.game_id).await?;
        transaction.commit().await?;
        Ok(ReadyGame {
            engine_revision: engine_revision.to_owned(),
            pending,
            pending_undo,
            chain_info_activation: room.chain_info_activation,
            global_reaction_reset: room.global_reaction_reset,
            wait_before_move: room.wait_before_move,
            time_war_separation: room.time_war_separation.map(|n| n as usize),
            auth: AuthenticatedPlayer {
                game_id: room.game_id,
                side,
                player_token,
            },
            revision,
            history,
        })
    }

    /// Store only a proposal. Commands are opaque to the server; no engine is linked.
    pub(crate) async fn propose_command(
        &self,
        auth: &AuthenticatedPlayer,
        client_revision: u64,
        command: &str,
    ) -> Result<CommandResult, RepositoryError> {
        if command.is_empty()
            || command.len() > MAX_COMMAND_LEN
            || !command.is_ascii()
            || command.contains(['\r', '\n'])
        {
            return Err(client_error("invalid_command", "Invalid command envelope."));
        }
        let mut transaction = self.pool.begin().await?;
        let room = locked_room(&mut transaction, &auth.game_id).await?;
        authenticate_as(&room, auth.side, &auth.player_token)?;
        let revision = revision_to_u64(room.revision, &room.game_id)?;
        if revision != client_revision {
            return sync_result(&mut transaction, &room).await;
        }
        if room.command_count as usize >= MAX_COMMANDS {
            return Err(client_error(
                "history_limit",
                "Limit reached: 10000 commands.",
            ));
        }
        if room.white_token_hash.is_none() || room.black_token_hash.is_none() {
            return Err(client_error(
                "opponent_missing",
                "Waiting for the other player.",
            ));
        }
        sqlx::query("DELETE FROM command_proposals WHERE game_id = $1 AND created_at <= now() - interval '5 minutes'")
            .bind(&room.game_id).execute(&mut *transaction).await?;
        if let Some(existing) = fetch_proposal(&mut transaction, &room.game_id).await? {
            if existing.undo_keep_commands.is_some() {
                return Err(client_error(
                    "proposal_pending",
                    "An undo request is awaiting confirmation.",
                ));
            }
            let proposal = existing.proposal()?;
            if proposal.side == auth.side && proposal.command == command {
                transaction.commit().await?;
                return Ok(CommandResult::Proposed(proposal));
            }
            return Err(client_error(
                "proposal_pending",
                "Another command is awaiting confirmation.",
            ));
        }
        let proposal = CommandProposal {
            id: Uuid::new_v4().simple().to_string(),
            revision,
            side: auth.side,
            command: command.to_owned(),
        };
        sqlx::query("INSERT INTO command_proposals (game_id, proposal_id, revision, side, command) VALUES ($1, $2, $3, $4, $5)")
            .bind(&room.game_id).bind(&proposal.id).bind(room.revision)
            .bind(side_text(auth.side)).bind(command).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(CommandResult::Proposed(proposal))
    }

    /// Each authenticated color has one vote. The second acceptance commits the
    /// command and revision atomically; duplicate/stale votes never append twice.
    pub(crate) async fn vote_command(
        &self,
        auth: &AuthenticatedPlayer,
        proposal_id: &str,
        client_revision: u64,
        accepted: bool,
        reason: Option<&str>,
    ) -> Result<CommandResult, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let room = locked_room(&mut transaction, &auth.game_id).await?;
        authenticate_as(&room, auth.side, &auth.player_token)?;
        let Some(mut row) = fetch_proposal(&mut transaction, &room.game_id).await? else {
            return Ok(CommandResult::Unchanged);
        };
        if row.proposal_id != proposal_id || row.undo_keep_commands.is_some() {
            return Ok(CommandResult::Unchanged);
        }
        if row.revision != room.revision
            || client_revision != revision_to_u64(room.revision, &room.game_id)?
        {
            return sync_result(&mut transaction, &room).await;
        }
        if !accepted {
            sqlx::query("DELETE FROM command_proposals WHERE game_id = $1")
                .bind(&room.game_id)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            return Ok(CommandResult::Rejected {
                proposal_id: proposal_id.to_owned(),
                side: auth.side,
                reason: reason
                    .unwrap_or("The client rejected the command.")
                    .chars()
                    .take(MAX_REJECTION_LEN)
                    .collect(),
            });
        }
        match auth.side {
            Side::White => row.white_accepted = true,
            Side::Black => row.black_accepted = true,
        }
        if !(row.white_accepted && row.black_accepted) {
            sqlx::query("UPDATE command_proposals SET white_accepted = $2, black_accepted = $3 WHERE game_id = $1")
                .bind(&room.game_id).bind(row.white_accepted).bind(row.black_accepted)
                .execute(&mut *transaction).await?;
            transaction.commit().await?;
            return Ok(CommandResult::Unchanged);
        }
        let next_revision = room
            .revision
            .checked_add(1)
            .ok_or_else(|| client_error("revision_overflow", "Game revision overflow."))?;
        sqlx::query("INSERT INTO game_commands (game_id, revision, command) VALUES ($1, $2, $3)")
            .bind(&room.game_id)
            .bind(next_revision)
            .bind(&row.command)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE games SET revision = $2, command_count = command_count + 1, updated_at = now() WHERE game_id = $1")
            .bind(&room.game_id)
            .bind(next_revision)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM command_proposals WHERE game_id = $1")
            .bind(&room.game_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(CommandResult::Accepted {
            revision: next_revision as u64,
            command: row.command,
        })
    }

    /// Only the clients interpret the prefix as a chess turn; the server stores
    /// the request under the same row lock and pending slot as ordinary commands.
    pub(crate) async fn request_undo(
        &self,
        auth: &AuthenticatedPlayer,
        client_revision: u64,
        keep_commands: usize,
    ) -> Result<CommandResult, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let room = locked_room(&mut transaction, &auth.game_id).await?;
        authenticate_as(&room, auth.side, &auth.player_token)?;
        let revision = revision_to_u64(room.revision, &room.game_id)?;
        if revision != client_revision {
            return sync_result(&mut transaction, &room).await;
        }
        if room.white_token_hash.is_none() || room.black_token_hash.is_none() {
            return Err(client_error(
                "opponent_missing",
                "Waiting for the other player.",
            ));
        }
        let count = room.command_count as usize;
        if keep_commands >= MAX_COMMANDS || keep_commands >= count {
            return Err(client_error("invalid_undo", "Invalid undo prefix."));
        }
        sqlx::query("DELETE FROM command_proposals WHERE game_id = $1 AND created_at <= now() - interval '5 minutes'")
            .bind(&room.game_id).execute(&mut *transaction).await?;
        if let Some(row) = fetch_proposal(&mut transaction, &room.game_id).await? {
            if row.undo_keep_commands == Some(keep_commands as i32) {
                let proposal = row.undo_proposal()?;
                if proposal.side == auth.side {
                    transaction.commit().await?;
                    return Ok(CommandResult::UndoProposed(proposal));
                }
            }
            return Err(client_error(
                "proposal_pending",
                "Another request is awaiting confirmation.",
            ));
        }
        let proposal = UndoProposal {
            id: Uuid::new_v4().simple().to_string(),
            revision,
            side: auth.side,
            keep_commands,
        };
        sqlx::query("INSERT INTO command_proposals (game_id, proposal_id, revision, side, command, undo_keep_commands) VALUES ($1, $2, $3, $4, '', $5)")
            .bind(&room.game_id).bind(&proposal.id).bind(room.revision)
            .bind(side_text(auth.side)).bind(keep_commands as i32)
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(CommandResult::UndoProposed(proposal))
    }

    pub(crate) async fn vote_undo(
        &self,
        auth: &AuthenticatedPlayer,
        proposal_id: &str,
        client_revision: u64,
        accepted: bool,
    ) -> Result<CommandResult, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let room = locked_room(&mut transaction, &auth.game_id).await?;
        authenticate_as(&room, auth.side, &auth.player_token)?;
        let Some(row) = fetch_proposal(&mut transaction, &room.game_id).await? else {
            return sync_result(&mut transaction, &room).await;
        };
        if row.proposal_id != proposal_id || row.undo_keep_commands.is_none() {
            return sync_result(&mut transaction, &room).await;
        }
        let proposal = row.undo_proposal()?;
        if proposal.revision != client_revision
            || revision_to_u64(room.revision, &room.game_id)? != client_revision
        {
            return sync_result(&mut transaction, &room).await;
        }
        // Requesting already represents consent. Only the other authenticated
        // seat can answer, regardless of whose turn it currently is.
        if proposal.side == auth.side {
            return Err(client_error(
                "invalid_undo_vote",
                "Only the opponent can answer an undo request.",
            ));
        }
        sqlx::query("DELETE FROM command_proposals WHERE game_id = $1")
            .bind(&room.game_id)
            .execute(&mut *transaction)
            .await?;
        if !accepted {
            transaction.commit().await?;
            return Ok(CommandResult::UndoDeclined {
                proposal_id: proposal.id,
            });
        }
        let next_revision = room
            .revision
            .checked_add(1)
            .ok_or_else(|| client_error("revision_overflow", "Game revision overflow."))?;
        // Journal revisions may have gaps after undo. Network revisions never
        // decrease, so pre-undo votes cannot commit against a later position.
        sqlx::query("DELETE FROM game_commands WHERE game_id = $1 AND revision IN (SELECT revision FROM game_commands WHERE game_id = $1 ORDER BY revision OFFSET $2)")
            .bind(&room.game_id).bind(proposal.keep_commands as i64)
            .execute(&mut *transaction).await?;
        sqlx::query("UPDATE games SET revision = $2, command_count = $3, updated_at = now() WHERE game_id = $1")
            .bind(&room.game_id)
            .bind(next_revision)
            .bind(proposal.keep_commands as i32)
            .execute(&mut *transaction)
            .await?;
        let history = fetch_history(&mut transaction, &room.game_id).await?;
        transaction.commit().await?;
        Ok(CommandResult::UndoAccepted {
            proposal_id: proposal.id,
            revision: next_revision as u64,
            history,
        })
    }

    pub(crate) async fn resync(
        &self,
        auth: &AuthenticatedPlayer,
    ) -> Result<CommandResult, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let room = locked_room(&mut transaction, &auth.game_id).await?;
        authenticate_as(&room, auth.side, &auth.player_token)?;
        let mut result = sync_result(&mut transaction, &room).await?;
        if let CommandResult::Sync { reason, .. } = &mut result {
            *reason = SyncReason::Requested;
        }
        transaction.commit().await?;
        Ok(result)
    }
}

#[derive(FromRow)]
struct ProposalRow {
    proposal_id: String,
    revision: i64,
    side: String,
    command: String,
    white_accepted: bool,
    black_accepted: bool,
    undo_keep_commands: Option<i32>,
}
impl ProposalRow {
    fn undo_proposal(self) -> Result<UndoProposal, RepositoryError> {
        let keep_commands = self
            .undo_keep_commands
            .ok_or_else(|| client_error("invalid_undo", "Not an undo proposal."))?
            as usize;
        let proposal = self.proposal()?;
        Ok(UndoProposal {
            id: proposal.id,
            revision: proposal.revision,
            side: proposal.side,
            keep_commands,
        })
    }

    fn proposal(self) -> Result<CommandProposal, RepositoryError> {
        let side = match self.side.as_str() {
            "white" => Side::White,
            "black" => Side::Black,
            _ => {
                return Err(client_error(
                    "server_error",
                    "Invalid stored proposal side.",
                ));
            }
        };
        Ok(CommandProposal {
            id: self.proposal_id,
            revision: self.revision as u64,
            side,
            command: self.command,
        })
    }
}

async fn pending_proposals(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    game_id: &str,
) -> Result<(Option<CommandProposal>, Option<UndoProposal>), RepositoryError> {
    match fetch_proposal(transaction, game_id).await? {
        Some(row) if row.undo_keep_commands.is_some() => Ok((None, Some(row.undo_proposal()?))),
        Some(row) => Ok((Some(row.proposal()?), None)),
        None => Ok((None, None)),
    }
}

async fn fetch_proposal(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    game_id: &str,
) -> Result<Option<ProposalRow>, sqlx::Error> {
    sqlx::query_as::<_, ProposalRow>("SELECT proposal_id, revision, side, command, white_accepted, black_accepted, undo_keep_commands FROM command_proposals WHERE game_id = $1 AND created_at > now() - interval '5 minutes'")
        .bind(game_id).fetch_optional(&mut **transaction).await
}

async fn sync_result(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    room: &RoomRow,
) -> Result<CommandResult, RepositoryError> {
    let history = fetch_history(transaction, &room.game_id).await?;
    verify_revision(room, &history)?;
    let (pending, pending_undo) = pending_proposals(transaction, &room.game_id).await?;
    Ok(CommandResult::Sync {
        revision: revision_to_u64(room.revision, &room.game_id)?,
        history,
        pending,
        pending_undo,
        reason: SyncReason::RevisionMismatch,
    })
}

fn side_text(side: Side) -> &'static str {
    match side {
        Side::White => "white",
        Side::Black => "black",
    }
}
fn validate_engine_revision(value: &str) -> Result<(), RepositoryError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(client_error(
            "invalid_engine_revision",
            "Invalid engine revision hash.",
        ));
    }
    Ok(())
}

async fn locked_room(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    game_id: &str,
) -> Result<RoomRow, RepositoryError> {
    sqlx::query_as::<_, RoomRow>(
        "SELECT game_id, white_token_hash, black_token_hash, revision, command_count, causal_gaps, chain_info_activation, global_reaction_reset, engine_revision, time_war_separation, wait_before_move
         FROM games WHERE game_id = $1 FOR UPDATE",
    )
    .bind(game_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| client_error("game_not_found", "The game no longer exists."))
}

async fn fetch_history(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    game_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT command FROM game_commands WHERE game_id = $1 ORDER BY revision ASC",
    )
    .bind(game_id)
    .fetch_all(&mut **transaction)
    .await
}

fn verify_revision(room: &RoomRow, history: &[String]) -> Result<(), RepositoryError> {
    let revision = revision_to_u64(room.revision, &room.game_id)?;
    if history.len() != room.command_count as usize || (history.len() as u64) > revision {
        return Err(RepositoryError::Corrupt {
            game_id: room.game_id.clone(),
            detail: format!(
                "revision {revision} does not match {} stored moves",
                history.len()
            ),
        });
    }
    Ok(())
}

fn authenticate(room: &RoomRow, token: &str) -> Result<Side, RepositoryError> {
    let hash = hash_token(token);
    match (
        room.white_token_hash.as_deref() == Some(hash.as_slice()),
        room.black_token_hash.as_deref() == Some(hash.as_slice()),
    ) {
        (true, false) => Ok(Side::White),
        (false, true) => Ok(Side::Black),
        _ => Err(client_error(
            "invalid_player_token",
            "The saved player token is invalid for this game.",
        )),
    }
}

fn authenticate_as(room: &RoomRow, side: Side, token: &str) -> Result<(), RepositoryError> {
    if authenticate(room, token)? == side {
        Ok(())
    } else {
        Err(client_error(
            "invalid_player_token",
            "The token does not own this color.",
        ))
    }
}

fn normalize_game_id(value: &str) -> Result<String, RepositoryError> {
    let value = value.trim().to_ascii_uppercase();
    if value.is_empty()
        || value.len() > MAX_GAME_ID_LEN
        || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(client_error("invalid_game_id", "Malformed game id."));
    }
    Ok(value)
}

fn generate_game_id() -> String {
    let random = Uuid::new_v4();
    random
        .as_bytes()
        .iter()
        .take(GAME_ID_LEN)
        .map(|byte| GAME_ID_ALPHABET[usize::from(*byte) % GAME_ID_ALPHABET.len()] as char)
        .collect()
}

fn generate_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn resolve_side(preference: SidePreference) -> Side {
    match preference {
        SidePreference::White => Side::White,
        SidePreference::Black => Side::Black,
        SidePreference::Random if Uuid::new_v4().as_bytes()[0] & 1 == 0 => Side::White,
        SidePreference::Random => Side::Black,
    }
}

fn revision_to_u64(revision: i64, game_id: &str) -> Result<u64, RepositoryError> {
    u64::try_from(revision).map_err(|_| RepositoryError::Corrupt {
        game_id: game_id.to_owned(),
        detail: format!("negative revision {revision}"),
    })
}

fn client_error(code: &'static str, message: impl Into<String>) -> RepositoryError {
    RepositoryError::Client {
        code,
        message: message.into(),
    }
}
