//! Room transport is non-Send (browser sockets), separate from the game resource.
//! Revisions identify commits and never decrease, including after agreed undo.
use crate::controller::{GameUi, side_name};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, futures::check_ready};
use bevy_egui::{EguiContexts, egui};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};
use temporal_chess_engine::{
    ENGINE_REVISION,
    engine::{Engine, LabSetup},
    model::{Color, GameRules, StartingPosition},
    record::{Command, GameRecord},
};
use temporal_chess_protocol::{
    ClientMessage, CommandProposal, MAX_COMMAND_LEN, MAX_COMMANDS, MAX_REJECTION_LEN,
    PROTOCOL_VERSION, ServerMessage, Side, SidePreference, UndoProposal,
};
use web_time::Instant;

pub struct OnlineState {
    pub rules: GameRules,
    pub starting_position: StartingPosition,
    pub lab_setup: Option<LabSetup>,
    pub enabled: bool,
    pub ready: bool,
    pub waiting: bool,
    pub synchronizing: bool,
    pub fatal: bool,
    pub side: Option<Color>,
    pub revision: u64,
    pub outgoing: Option<Command>,
    pub status: crate::i18n::Message,
    pub pending_undo: Option<UndoProposal>,
    undo_answered: bool,
    undo_notice: Option<crate::i18n::Message>,
    endpoint: String,
    room_input: String,
    room: Option<String>,
    preference: SidePreference,
    opponent: bool,
    action: Option<Action>,
}
impl Default for OnlineState {
    fn default() -> Self {
        Self {
            rules: GameRules::default(),
            starting_position: StartingPosition::Standard,
            lab_setup: None,
            enabled: false,
            ready: false,
            waiting: false,
            synchronizing: false,
            fatal: false,
            side: None,
            revision: 0,
            outgoing: None,
            status: Default::default(),
            pending_undo: None,
            undo_answered: false,
            undo_notice: None,
            endpoint: default_endpoint(),
            room_input: invite_room(),
            room: None,
            preference: SidePreference::Random,
            opponent: false,
            action: None,
        }
    }
}
impl OnlineState {
    pub fn restart_local(
        &mut self,
        rules: GameRules,
        start: StartingPosition,
        bot: crate::tempfish::Settings,
    ) {
        self.action = Some(Action::LocalGame { rules, start, bot });
    }
    pub fn initial_record(&self) -> GameRecord {
        if self.starting_position == StartingPosition::Lab
            && let Some(setup) = &self.lab_setup
        {
            return GameRecord::with_lab_setup(setup.clone());
        }
        GameRecord::with_start(self.rules, self.starting_position)
    }
    pub fn create_from_lab(&mut self, record: &GameRecord) {
        self.enabled = true;
        self.ready = false;
        self.lab_setup = record.lab_setup().cloned();
        self.rules = record.rules();
        self.starting_position = StartingPosition::Lab;
        self.action = Some(Action::Create);
    }
    pub fn opponent_connected(&self) -> bool {
        self.opponent
    }

    pub fn request_undo(&mut self, keep_commands: usize) {
        self.waiting = true;
        self.undo_notice = None;
        self.status = crate::message!("Запрашиваем отмену хода…", "Requesting undo…");
        self.action = Some(Action::RequestUndo { keep_commands });
    }

    fn answer_undo(&mut self, accepted: bool) {
        if let Some(proposal) = self.pending_undo.clone()
            && !self.undo_answered
            && self.side != Some(color(proposal.side))
        {
            self.undo_answered = true;
            self.action = Some(Action::UndoVote { proposal, accepted });
        }
    }
}

fn color(side: Side) -> Color {
    match side {
        Side::White => Color::White,
        Side::Black => Color::Black,
    }
}

fn undo_prefix(record: &GameRecord) -> Option<usize> {
    record
        .commands()
        .iter()
        .rposition(|command| command.spends_turn(record.rules()))
}
enum Action {
    LocalGame {
        rules: GameRules,
        start: StartingPosition,
        bot: crate::tempfish::Settings,
    },
    Create,
    Join,
    Retry,
    Leave,
    RequestUndo {
        keep_commands: usize,
    },
    UndoVote {
        proposal: UndoProposal,
        accepted: bool,
    },
}

struct PreparedCommand {
    command: Command,
    engine: Engine,
    record: GameRecord,
}
struct ValidationJob {
    proposal: CommandProposal,
    task: Task<Result<PreparedCommand, String>>,
}

fn validate_proposal(
    mut engine: Engine,
    mut record: GameRecord,
    proposal: &CommandProposal,
    expected_revision: u64,
) -> Result<PreparedCommand, String> {
    if expected_revision != proposal.revision {
        return Err("Game revision mismatch.".into());
    }
    let side = match proposal.side {
        Side::White => Color::White,
        Side::Black => Color::Black,
    };
    if engine.turn() != side {
        return Err("piece cannot act on this turn".into());
    }
    let command: Command = proposal.command.parse()?;
    // Bounds belong to the clients too: the server does not parse chess commands.
    let source = match command {
        Command::Move { mv, .. } | Command::MoveAndWait { mv, .. } => Some(mv.source()),
        Command::Advance { source, to_t, .. } => {
            if to_t < engine.history().beginning() || to_t > engine.history().horizon() {
                return Err("board is unavailable".into());
            }
            Some(source)
        }
        Command::Continue | Command::DeclineReactions => None,
    };
    if source.is_some_and(|source| {
        source.t < engine.history().beginning()
            || source.t > engine.history().horizon()
            || source.piece.wid >= engine.history().worldlines().len()
            || (source.piece.fut_seen > engine.history().horizon()
                && engine.history().starting_position() != StartingPosition::Lab)
            || source.piece.loop_turn > MAX_COMMANDS
    }) {
        return Err("piece occurrence is absent or cannot act".into());
    }
    record
        .apply(&mut engine, command)
        .map_err(|error| error.to_string())?;
    Ok(PreparedCommand {
        command,
        engine,
        record,
    })
}

#[derive(Default)]
pub struct MultiplayerClient {
    sender: Option<WsSender>,
    receiver: Option<WsReceiver>,
    handshake: Option<ClientMessage>,
    token: Option<String>,
    tokens: HashMap<String, String>,
    accepted: VecDeque<(u64, String)>,
    received_revision: u64,
    awaiting_snapshot: bool,
    retry_at: Option<Instant>,
    deadline: Option<Instant>,
    retry_delay: u64,
    proposal: Option<CommandProposal>,
    validation: Option<ValidationJob>,
    prepared: Option<(CommandProposal, PreparedCommand)>,
    voted: Option<String>,
}
impl MultiplayerClient {
    fn close(&mut self) {
        if let Some(mut sender) = self.sender.take() {
            sender.close();
        }
        self.receiver = None;
        self.handshake = None;
        self.deadline = None;
        self.proposal = None;
        self.validation = None;
        self.prepared = None;
        self.voted = None;
    }
    fn connect(
        &mut self,
        url: &str,
        handshake: ClientMessage,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        self.close();
        self.retry_at = None;
        self.accepted.clear();
        self.awaiting_snapshot = true;
        let wake = ctx.clone();
        let (sender, receiver) = ewebsock::connect_with_wakeup(
            url,
            ewebsock::Options {
                max_incoming_frame_size: temporal_chess_protocol::MAX_LAB_SETUP_LEN
                    + 4 * 1024 * 1024,
                ..Default::default()
            },
            move || wake.request_repaint(),
        )
        .map_err(|e| e.to_string())?;
        self.sender = Some(sender);
        self.receiver = Some(receiver);
        self.handshake = Some(handshake);
        self.deadline = Some(Instant::now() + Duration::from_secs(15));
        Ok(())
    }
    fn send(&mut self, message: &ClientMessage) {
        if let Some(sender) = &mut self.sender {
            sender.send(WsMessage::Text(
                serde_json::to_string(message).expect("protocol JSON"),
            ));
        }
    }
    fn disconnected(&mut self, game: &mut GameUi) {
        self.close();
        game.online.ready = false;
        game.online.pending_undo = None;
        game.online.undo_answered = false;
        if matches!(
            game.online.action,
            Some(Action::RequestUndo { .. } | Action::UndoVote { .. })
        ) {
            game.online.action = None;
        }
        game.online.outgoing = None;
        game.pending_move = None;
        game.pending_square = None;
        game.clear_selection();
        if game.remote_focus.take().is_some() {
            game.playback_turn = None;
            game.jump_to = None;
            game.viewport.scroll = None;
        }
        if game.online.fatal {
            return;
        }
        if game.online.room.is_some() && self.token.is_some() {
            self.retry_delay = (self.retry_delay.max(1) * 2).min(20);
            self.retry_at = Some(Instant::now() + Duration::from_secs(self.retry_delay));
            game.online.status = crate::message!(
                "Связь потеряна. Переподключение…",
                "Connection lost. Reconnecting…"
            );
        } else {
            game.online.fatal = true;
            game.online.status = crate::message!(
                "Не удалось подключиться. Проверьте адрес сервера.",
                "Connection failed. Check the server address."
            );
        }
    }
    fn resync(&mut self, game: &mut GameUi) {
        game.reset_playback();
        game.jump_to = None;
        game.viewport.scroll = None;
        game.online.synchronizing = true;
        game.online.waiting = true;
        game.online.outgoing = None;
        self.accepted.clear();
        self.proposal = None;
        self.validation = None;
        self.prepared = None;
        self.voted = None;
        self.awaiting_snapshot = true;
        self.send(&ClientMessage::Resync {
            revision: game.online.revision,
        });
        self.deadline = Some(Instant::now() + Duration::from_secs(30));
    }

    fn poll_validation(&mut self, game: &mut GameUi) {
        if let Some(job) = &mut self.validation
            && let Some(result) = check_ready(&mut job.task)
        {
            let proposal = self.validation.take().unwrap().proposal;
            let current = self.proposal.as_ref().is_some_and(|p| p.id == proposal.id);
            let committed = self.accepted.iter().any(|(revision, command)| {
                *revision == proposal.revision + 1 && *command == proposal.command
            });
            if current || committed {
                let reason = result
                    .as_ref()
                    .err()
                    .map(|error| error.chars().take(MAX_REJECTION_LEN).collect());
                if current {
                    self.send(&ClientMessage::CommandVote {
                        proposal_id: proposal.id.clone(),
                        revision: proposal.revision,
                        accepted: result.is_ok(),
                        reason,
                    });
                    self.voted = Some(proposal.id.clone());
                    self.deadline = Some(Instant::now() + Duration::from_secs(300));
                }
                if let Ok(prepared) = result {
                    self.prepared = Some((proposal, prepared));
                } else if committed {
                    self.resync(game);
                }
            }
        }
        if self.validation.is_none()
            && self.accepted.is_empty()
            && let Some(proposal) = &self.proposal
            && proposal.revision == game.online.revision
            && self.voted.as_ref() != Some(&proposal.id)
        {
            let engine = game.engine.clone();
            let record = game.record.clone();
            let proposed = proposal.clone();
            let revision = game.online.revision;
            let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
                std::panic::catch_unwind(|| validate_proposal(engine, record, &proposed, revision))
                    .map_err(|_| "Engine panicked while validating the command.".to_owned())?
            });
            self.validation = Some(ValidationJob {
                proposal: proposal.clone(),
                task,
            });
            game.online.waiting = true;
        }
    }
}

pub fn poll(
    mut client: NonSendMut<MultiplayerClient>,
    mut game: ResMut<GameUi>,
    mut contexts: EguiContexts,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if let Some(action) = game.online.action.take() {
        match action {
            Action::LocalGame { rules, start, bot } => {
                client.close();
                client.retry_at = None;
                client.accepted.clear();
                game.start_new_local(rules, start, bot);
                return;
            }
            Action::RequestUndo { keep_commands } => {
                client.send(&ClientMessage::RequestUndo {
                    revision: game.online.revision,
                    keep_commands,
                });
                client.deadline = Some(Instant::now() + Duration::from_secs(300));
            }
            Action::UndoVote { proposal, accepted } => {
                client.send(&ClientMessage::UndoVote {
                    proposal_id: proposal.id,
                    revision: proposal.revision,
                    accepted,
                });
                client.deadline = Some(Instant::now() + Duration::from_secs(30));
            }
            Action::Leave => {
                client.close();
                client.retry_at = None;
                client.accepted.clear();
                game.replay_job = None;
                game.reset_playback();
                game.viewport.scroll = None;
                game.jump_to = None;
                game.clear_selection();
                game.pending_move = None;
                game.pending_square = None;
                game.online = OnlineState {
                    endpoint: game.online.endpoint.clone(),
                    ..Default::default()
                };
                return;
            }
            Action::Create | Action::Join | Action::Retry => {
                let retry = matches!(action, Action::Retry);
                let handshake = if matches!(action, Action::Create) {
                    client.token = None;
                    game.online.room = None;
                    ClientMessage::CreateGame {
                        engine_revision: ENGINE_REVISION.to_owned(),
                        lab_setup: if game.online.starting_position == StartingPosition::Lab {
                            game.online.lab_setup.as_ref().map(LabSetup::to_json)
                        } else {
                            None
                        },
                        time_war_separation: game
                            .online
                            .starting_position
                            .initial_end()
                            .map(|t| t as usize),
                        chain_info_activation: game.online.rules.chain_info_activation,
                        global_reaction_reset: game.online.rules.global_reaction_reset,
                        wait_before_move: game.online.rules.wait_before_move,
                        protocol: PROTOCOL_VERSION,
                        side: game.online.preference,
                    }
                } else {
                    let room = if retry {
                        game.online
                            .room
                            .clone()
                            .unwrap_or_else(|| game.online.room_input.clone())
                    } else {
                        game.online.room_input.trim().to_ascii_uppercase()
                    };
                    let key = token_key(&game.online.endpoint, &room);
                    client.token = client
                        .tokens
                        .get(&key)
                        .cloned()
                        .or_else(|| load_token(&key));
                    game.online.room = Some(room.clone());
                    ClientMessage::JoinGame {
                        engine_revision: ENGINE_REVISION.to_owned(),
                        protocol: PROTOCOL_VERSION,
                        game_id: room,
                        player_token: client.token.clone(),
                    }
                };
                game.online.enabled = true;
                game.online.ready = false;
                game.online.waiting = false;
                game.online.synchronizing = true;
                game.online.fatal = false;
                game.online.outgoing = None;
                game.online.pending_undo = None;
                game.online.undo_answered = false;
                game.online.status = crate::message!("Подключение…", "Connecting…");
                game.pending_square = None;
                game.pending_move = None;
                game.clear_selection();
                if let Err(error) = client.connect(&game.online.endpoint, handshake, ctx) {
                    client.disconnected(&mut game);
                    if game.online.fatal {
                        game.online.status = error.into();
                    }
                }
            }
        }
    }
    if !game.online.enabled {
        return;
    }
    // Also wakes reactive Bevy windows while waiting for retries/timeouts.
    ctx.request_repaint_after(Duration::from_millis(100));
    if game.online.fatal {
        if game.remote_focus.take().is_some() {
            game.playback_turn = None;
            game.jump_to = None;
            game.viewport.scroll = None;
        }
        client.close();
        client.retry_at = None;
        return;
    }
    if client.retry_at.is_some_and(|at| Instant::now() >= at) {
        game.online.action = Some(Action::Retry);
        client.retry_at = None;
        return;
    }
    if client.deadline.is_some_and(|at| Instant::now() >= at) {
        client.disconnected(&mut game);
        return;
    }
    // Limit messages processed per frame; animations must remain responsive.
    for _ in 0..32 {
        let Some(event) = client.receiver.as_ref().and_then(WsReceiver::try_recv) else {
            break;
        };
        match event {
            WsEvent::Opened => {
                if let Some(handshake) = client.handshake.take() {
                    client.send(&handshake);
                }
            }
            WsEvent::Message(WsMessage::Text(text)) => {
                match serde_json::from_str::<ServerMessage>(&text) {
                    Ok(message) => receive(&mut client, &mut game, message),
                    Err(_) => {
                        game.online.fatal = true;
                        game.online.ready = false;
                        game.online.status = crate::message!(
                            "Неподдерживаемый ответ сервера. Обновите клиент.",
                            "Unsupported server response. Update the client."
                        );
                    }
                }
            }
            WsEvent::Closed | WsEvent::Error(_) => {
                client.disconnected(&mut game);
                break;
            }
            _ => {}
        }
    }
    if game.online.fatal || !game.online.ready {
        return;
    }
    if game.online.synchronizing || game.replay_job.is_some() {
        return;
    }
    client.poll_validation(&mut game);
    if !game.history_playback
        && !game.replaying_move
        && client.validation.is_none()
        && game.transition.is_none()
        && game.transition_queue.is_empty()
        && game.cancellations.is_empty()
        && !game.returning_camera
        && !game.facing.animating()
    {
        if let Some((revision, text)) = client.accepted.front()
            && let Ok(command) = text.parse::<Command>()
            && let Some(mv) = command.spatial_move()
            && game.online.side != Some(game.engine.turn())
        {
            if game.remote_focus != Some(*revision) {
                game.remote_focus = Some(*revision);
                game.playback_turn = Some(game.engine.turn());
                game.viewport.scroll = None;
                game.jump_to = Some(mv.t);
                return;
            }
            if game.jump_to.is_some() || game.viewport.scroll.is_some() {
                return;
            }
        }
        if let Some((revision, text)) = client.accepted.pop_front() {
            if revision != game.online.revision + 1 {
                client.resync(&mut game);
                return;
            }
            let result = if client.prepared.as_ref().is_some_and(|(proposal, _)| {
                proposal.revision + 1 == revision && proposal.command == text
            }) {
                let (_, prepared) = client.prepared.take().unwrap();
                game.apply_prepared(prepared.command, prepared.engine, prepared.record);
                Ok(())
            } else {
                text.parse::<Command>()
                    .and_then(|command| game.apply_confirmed(command))
            };
            if result.is_err() {
                // A different engine build must not loop on the same invalid journal.
                client.resync(&mut game);
                return;
            }
            game.online.revision = revision;
            if client.accepted.is_empty() {
                game.online.waiting =
                    client.proposal.is_some() || game.online.pending_undo.is_some();
            }
        }
    }
    if let Some(proposal) = game.online.pending_undo.clone()
        && proposal.revision == game.online.revision
        && !game.online.undo_answered
        && game.online.side != Some(color(proposal.side))
        && undo_prefix(&game.record) != Some(proposal.keep_commands)
    {
        // A peer cannot use the consent dialog to truncate arbitrary history.
        game.online.answer_undo(false);
    }
    if let Some(command) = game.online.outgoing.take() {
        let revision = game.online.revision;
        client.send(&ClientMessage::PlayCommand {
            revision,
            command: command.to_string(),
        });
        client.deadline = Some(Instant::now() + Duration::from_secs(300));
    }
}

fn receive(client: &mut MultiplayerClient, game: &mut GameUi, message: ServerMessage) {
    match message {
        ServerMessage::GameReady {
            time_war_separation,
            lab_setup,
            engine_revision,
            pending,
            pending_undo,
            chain_info_activation,
            global_reaction_reset,
            wait_before_move,
            game_id,
            player_token,
            side,
            revision,
            history,
            opponent_connected,
            ..
        } => {
            if engine_revision != ENGINE_REVISION {
                game.online.fatal = true;
                game.online.ready = false;
                game.online.status = crate::i18n::server_error("engine_revision_mismatch", "");
                return;
            }
            game.online.starting_position = time_war_separation
                .map_or(StartingPosition::Standard, |separation| {
                    StartingPosition::TimeWar { separation }
                });
            if !game.online.starting_position.is_valid() {
                game.online.fatal = true;
                game.online.ready = false;
                game.online.status = crate::message!(
                    "Некорректная стартовая позиция комнаты",
                    "Invalid room starting position"
                );
                return;
            }
            game.online.rules = GameRules {
                chain_info_activation,
                global_reaction_reset,
                wait_before_move,
            };
            game.online.lab_setup = match lab_setup {
                Some(json) => match LabSetup::from_json(&json) {
                    Ok(setup)
                        if setup.rules() == game.online.rules && time_war_separation.is_none() =>
                    {
                        game.online.starting_position = StartingPosition::Lab;
                        Some(setup)
                    }
                    _ => {
                        game.online.fatal = true;
                        game.online.ready = false;
                        game.online.status =
                            crate::message!("Некорректный сеттинг Lab", "Invalid Lab setting");
                        return;
                    }
                },
                None => None,
            };
            game.lab = None;
            let key = token_key(&game.online.endpoint, &game_id);
            client.tokens.insert(key.clone(), player_token.clone());
            save_token(&key, &player_token);
            client.token = Some(player_token);
            game.online.room_input = game_id.clone();
            game.online.room = Some(game_id);
            game.online.side = Some(match side {
                Side::White => Color::White,
                Side::Black => Color::Black,
            });
            game.online.opponent = opponent_connected;
            game.online.ready = true;
            game.online.status.clear();
            client.retry_delay = 0;
            snapshot(client, game, revision, history, pending, pending_undo);
        }
        ServerMessage::Sync {
            revision,
            history,
            pending,
            pending_undo,
            ..
        } => snapshot(client, game, revision, history, pending, pending_undo),
        ServerMessage::CommandProposed { proposal } => receive_proposal(client, game, proposal),
        ServerMessage::UndoProposed { proposal } => receive_undo(client, game, proposal),
        ServerMessage::UndoDeclined { proposal_id } => {
            if game
                .online
                .pending_undo
                .as_ref()
                .is_some_and(|p| p.id == proposal_id)
            {
                let requested = game
                    .online
                    .pending_undo
                    .as_ref()
                    .is_some_and(|p| game.online.side == Some(color(p.side)));
                game.online.pending_undo = None;
                game.online.undo_answered = false;
                if matches!(game.online.action, Some(Action::UndoVote { .. })) {
                    game.online.action = None;
                }
                game.online.waiting = !client.accepted.is_empty() || client.proposal.is_some();
                client.deadline = None;
                if requested {
                    let notice = crate::message!(
                        "Соперник отказал в отмене хода.",
                        "Your opponent declined the undo request."
                    );
                    game.online.status = notice.clone();
                    game.online.undo_notice = Some(notice);
                }
            }
        }
        ServerMessage::UndoAccepted {
            proposal_id,
            revision,
            history,
        } => {
            if client.awaiting_snapshot || revision <= client.received_revision {
                return;
            }
            if revision != client.received_revision + 1
                || !game
                    .online
                    .pending_undo
                    .as_ref()
                    .is_some_and(|p| p.id == proposal_id)
            {
                client.resync(game);
                return;
            }
            snapshot(client, game, revision, history, None, None);
            game.online.status = crate::message!(
                "Ход отменён по согласию игроков.",
                "Move undone by mutual agreement."
            );
        }
        ServerMessage::CommandRejected {
            proposal_id,
            reason,
            ..
        } => {
            if !client
                .proposal
                .as_ref()
                .is_some_and(|p| p.id == proposal_id)
            {
                return;
            }
            if client
                .proposal
                .as_ref()
                .is_some_and(|p| p.id == proposal_id)
            {
                client.proposal = None;
            }
            if client
                .validation
                .as_ref()
                .is_some_and(|job| job.proposal.id == proposal_id)
            {
                client.validation = None;
            }
            if client
                .prepared
                .as_ref()
                .is_some_and(|(p, _)| p.id == proposal_id)
            {
                client.prepared = None;
            }
            if client.voted.as_ref() == Some(&proposal_id) {
                client.voted = None;
            }
            game.online.waiting = !client.accepted.is_empty()
                || client.proposal.is_some()
                || game.online.pending_undo.is_some();
            client.deadline = None;
            game.online.status = crate::i18n::Message::with_error(
                "Команда отклонена клиентом: ",
                "Command rejected by a client: ",
                reason,
            );
        }
        ServerMessage::CommandAccepted { revision, command } => {
            if client.awaiting_snapshot || revision <= client.received_revision {
                return;
            }
            if revision != client.received_revision + 1
                || command.len() > MAX_COMMAND_LEN
                || client.accepted.len() >= MAX_COMMANDS
            {
                client.resync(game);
                return;
            }
            if client
                .proposal
                .as_ref()
                .is_some_and(|p| p.revision + 1 == revision && p.command == command)
            {
                client.proposal = None;
                client.voted = None;
            }
            client.received_revision = revision;
            client.accepted.push_back((revision, command));
            game.online.status.clear();
            game.online.waiting = true;
            client.deadline = None;
            game.clear_selection();
        }
        ServerMessage::OpponentConnection { connected } => game.online.opponent = connected,
        ServerMessage::Error { code, message } => {
            if code == "proposal_pending" {
                client.resync(game);
                return;
            }
            game.online.outgoing = None;
            game.online.waiting = !client.accepted.is_empty()
                || client.proposal.is_some()
                || game.online.pending_undo.is_some();
            if client.proposal.is_none() && game.online.pending_undo.is_none() {
                client.deadline = None;
            }
            game.online.status = crate::i18n::server_error(&code, &message);
            if !game.online.ready
                || matches!(
                    code.as_str(),
                    "connection_replaced"
                        | "invalid_player_token"
                        | "protocol_mismatch"
                        | "engine_revision_mismatch"
                        | "invalid_engine_revision"
                        | "game_not_found"
                        | "game_full"
                        | "removed_room_rules"
                )
            {
                game.online.fatal = true;
                game.online.ready = false;
            }
        }
    }
}

fn receive_proposal(client: &mut MultiplayerClient, game: &mut GameUi, proposal: CommandProposal) {
    if client.awaiting_snapshot || proposal.revision < client.received_revision {
        return;
    }
    if proposal.revision != client.received_revision
        || proposal.id.len() != 32
        || !proposal.id.bytes().all(|b| b.is_ascii_hexdigit())
        || proposal.command.len() > MAX_COMMAND_LEN
        || !proposal.command.is_ascii()
        || proposal.command.contains(['\r', '\n'])
    {
        client.resync(game);
        return;
    }
    if client
        .proposal
        .as_ref()
        .is_some_and(|p| p.id == proposal.id)
    {
        return;
    }
    if client
        .validation
        .as_ref()
        .is_some_and(|job| job.proposal.revision == proposal.revision)
    {
        client.validation = None;
    }
    if client
        .prepared
        .as_ref()
        .is_some_and(|(p, _)| p.revision == proposal.revision)
    {
        client.prepared = None;
    }
    client.voted = None;
    client.proposal = Some(proposal);
    game.online.waiting = true;
    game.online.status.clear();
    game.clear_selection();
    client.deadline = Some(Instant::now() + Duration::from_secs(300));
}

fn receive_undo(client: &mut MultiplayerClient, game: &mut GameUi, proposal: UndoProposal) {
    if client.awaiting_snapshot || proposal.revision < client.received_revision {
        return;
    }
    if proposal.revision != client.received_revision
        || proposal.id.len() != 32
        || !proposal.id.bytes().all(|b| b.is_ascii_hexdigit())
        || proposal.keep_commands >= MAX_COMMANDS
        || client.proposal.is_some()
    {
        client.resync(game);
        return;
    }
    if game
        .online
        .pending_undo
        .as_ref()
        .is_some_and(|p| p.id == proposal.id)
    {
        return;
    }
    game.online.pending_undo = Some(proposal);
    game.online.undo_answered = false;
    game.online.undo_notice = None;
    game.online.status.clear();
    game.online.waiting = true;
    game.online.outgoing = None;
    game.clear_selection();
    client.deadline = Some(Instant::now() + Duration::from_secs(300));
}

fn snapshot(
    client: &mut MultiplayerClient,
    game: &mut GameUi,
    revision: u64,
    history: Vec<String>,
    pending: Option<CommandProposal>,
    pending_undo: Option<UndoProposal>,
) {
    if revision < history.len() as u64
        || (pending.is_some() && pending_undo.is_some())
        || history.len() > MAX_COMMANDS
        || history
            .iter()
            .any(|line| line.len() > MAX_COMMAND_LEN || line.contains(['\r', '\n']))
    {
        game.online.fatal = true;
        game.online.ready = false;
        game.online.status = crate::message!(
            "Некорректный журнал сервера.",
            "Invalid server game record."
        );
        return;
    }
    client.deadline = None;
    client.accepted.clear();
    client.proposal = None;
    client.validation = None;
    client.prepared = None;
    client.voted = None;
    client.received_revision = revision;
    client.awaiting_snapshot = false;
    game.online.revision = revision;
    game.online.waiting = false;
    game.online.pending_undo = None;
    game.online.undo_answered = false;
    game.online.undo_notice = None;
    if matches!(
        game.online.action,
        Some(Action::RequestUndo { .. } | Action::UndoVote { .. })
    ) {
        game.online.action = None;
    }
    game.online.outgoing = None;
    game.online.synchronizing = true;
    game.begin_network_replay(&history);
    if let Some(proposal) = pending {
        receive_proposal(client, game, proposal);
    }
    if let Some(proposal) = pending_undo {
        receive_undo(client, game, proposal);
    }
}

/// Outside the collapsible sidebar, so incoming requests remain visible on phones.
pub fn undo_notification(ctx: &egui::Context, game: &mut GameUi) {
    if !game.online.enabled || !game.online.ready || game.online.fatal {
        return;
    }
    let width = 340.0_f32.min((ctx.viewport_rect().width() - 32.0).max(120.0));
    if let Some(proposal) = game.online.pending_undo.clone() {
        let incoming = game.online.side != Some(color(proposal.side));
        let ready = game.online.revision == proposal.revision
            && !game.online.synchronizing
            && game.replay_job.is_none()
            && !game.playback_busy()
            && !game.online.undo_answered
            && undo_prefix(&game.record) == Some(proposal.keep_commands);
        egui::Window::new(crate::tr!("Отмена хода", "Undo move"))
            .id("online_undo_request".into())
            .collapsible(false)
            .resizable(false)
            .default_width(width)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 28.0))
            .show(ctx, |ui| {
                ui.set_max_width(width);
                ui.label(if incoming {
                    crate::tr!(
                        "Соперник просит отменить последний ход. Отменить его для обоих игроков?",
                        "Your opponent requests undoing the last move. Undo it for both players?"
                    )
                } else {
                    crate::tr!(
                        "Запрос отмены отправлен. Ожидаем решения соперника…",
                        "Undo requested. Waiting for your opponent…"
                    )
                });
                if incoming {
                    ui.add_enabled_ui(ready, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button(crate::tr!("Согласиться", "Accept")).clicked() {
                                game.online.answer_undo(true);
                            }
                            if ui.button(crate::tr!("Отказать", "Decline")).clicked() {
                                game.online.answer_undo(false);
                            }
                        });
                    });
                }
                if !game.online.opponent_connected() {
                    ui.label(crate::tr!(
                        "Соперник отключился. Ожидаем подключения…",
                        "Opponent disconnected. Waiting for reconnection…"
                    ));
                }
            });
    } else if let Some(notice) = game.online.undo_notice.clone() {
        egui::Window::new(crate::tr!("Отмена хода", "Undo move"))
            .id("online_undo_notice".into())
            .collapsible(false)
            .resizable(false)
            .default_width(width)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 28.0))
            .show(ctx, |ui| {
                ui.set_max_width(width);
                ui.label(notice.text());
                if ui.button("OK").clicked() {
                    game.online.undo_notice = None;
                }
            });
    }
}

pub fn lab_controls(ui: &mut egui::Ui, online: &mut OnlineState) {
    ui.label(crate::tr!("Адрес сервера", "Server URL"));
    ui.text_edit_singleline(&mut online.endpoint);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(
            &mut online.preference,
            SidePreference::Random,
            crate::tr!("Случайная сторона", "Random side"),
        );
        ui.selectable_value(
            &mut online.preference,
            SidePreference::White,
            crate::tr!("Белые", "White"),
        );
        ui.selectable_value(
            &mut online.preference,
            SidePreference::Black,
            crate::tr!("Чёрные", "Black"),
        );
    });
}

pub fn creation_panel(ui: &mut egui::Ui, game: &mut GameUi) {
    let idle = game.replay_job.is_none() && !game.playback_busy();
    ui.add_space(8.0);
    if game.online.enabled {
        ui.strong(crate::tr!("Игра по сети", "Online game"));
        if let Some(room) = &game.online.room {
            ui.horizontal(|ui| {
                ui.monospace(room);
                if ui
                    .small_button(crate::tr!("Копировать код", "Copy code"))
                    .clicked()
                {
                    ui.ctx().copy_text(room.clone());
                }
            });
        }
        if let Some(side) = game.online.side {
            ui.label(crate::tr_format!(
                "Вы играете: {}",
                "You are playing: {}",
                side_name(side)
            ));
        }
        if !game.online.status.is_empty() {
            ui.label(game.online.status.text());
        }
        if game.online.ready {
            ui.label(if game.online.synchronizing {
                crate::tr!("Восстановление партии…", "Restoring game…")
            } else if game.playback_busy() {
                crate::tr!("Воспроизведение хода…", "Playing move animation…")
            } else if game.online.waiting {
                crate::tr!("Ожидание подтверждения…", "Waiting for confirmation…")
            } else if game.online.opponent {
                crate::tr!("Соперник подключён", "Opponent connected")
            } else {
                crate::tr!("Ожидание соперника", "Waiting for opponent")
            });
        }
        ui.horizontal(|ui| {
            if game.online.fatal
                && ui
                    .button(crate::tr!("Подключиться снова", "Reconnect"))
                    .clicked()
            {
                game.online.action = Some(Action::Retry);
            }
            if ui
                .button(crate::tr!(
                    "Выйти в локальный режим",
                    "Return to local play"
                ))
                .clicked()
            {
                game.online.action = Some(Action::Leave);
                game.confirm_restart = false;
                game.new_game_online = false;
            }
        });
        ui.small(crate::tr!(
            "Отмена хода требует согласия соперника.",
            "Undo requires your opponent’s consent."
        ));
    } else {
        ui.push_id("new_online_game", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.label(crate::tr!("Создать новую комнату", "Create a new room"));
                crate::view::starting_position_controls(ui, &mut game.new_game_start);
                crate::view::rule_controls(ui, &mut game.new_game_rules);
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut game.online.preference,
                        SidePreference::Random,
                        crate::tr!("Случайно", "Random"),
                    );
                    ui.selectable_value(
                        &mut game.online.preference,
                        SidePreference::White,
                        crate::tr!("Белые", "White"),
                    );
                    ui.selectable_value(
                        &mut game.online.preference,
                        SidePreference::Black,
                        crate::tr!("Чёрные", "Black"),
                    );
                });
                if ui
                    .add_enabled(
                        game.new_game_start.is_valid(),
                        egui::Button::new(crate::tr!("Создать комнату", "Create room")),
                    )
                    .clicked()
                {
                    game.online.rules = game.new_game_rules;
                    game.online.starting_position = game.new_game_start;
                    if game.online.starting_position == StartingPosition::Lab {
                        game.open_network_lab();
                    } else {
                        game.online.action = Some(Action::Create);
                    }
                }
                ui.separator();
                ui.add(
                    egui::TextEdit::singleline(&mut game.online.room_input)
                        .hint_text(crate::tr!("Код комнаты", "Room code"))
                        .char_limit(12),
                );
                if ui
                    .add_enabled(
                        !game.online.room_input.trim().is_empty(),
                        egui::Button::new(crate::tr!("Присоединиться", "Join")),
                    )
                    .clicked()
                {
                    game.online.action = Some(Action::Join);
                }
                egui::CollapsingHeader::new(crate::tr!("Сервер", "Server")).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut game.online.endpoint)
                            .hint_text("wss://example.com/ws"),
                    );
                });
                ui.small(crate::tr!(
                    "При подключении текущую позицию заменит сетевая партия.",
                    "Connecting will replace the current position with the online game."
                ));
            });
        });
    }
    ui.separator();
}

fn default_endpoint() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("NOLAN_WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:8080/ws".into())
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(url) = option_env!("NOLAN_WS_URL") {
            return url.into();
        }
        let location = web_sys::window().expect("browser window").location();
        let scheme = if location.protocol().ok().as_deref() == Some("https:") {
            "wss"
        } else {
            "ws"
        };
        format!("{scheme}://{}/ws", location.host().unwrap_or_default())
    }
}
fn invite_room() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.location().search().ok())
            .and_then(|search| {
                search
                    .trim_start_matches('?')
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("room=").map(str::to_owned))
            })
            .filter(|s| s.len() <= 12 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        String::new()
    }
}
fn token_key(endpoint: &str, room: &str) -> String {
    format!("nolan-chess.player.{endpoint}.{room}")
}
#[cfg(target_arch = "wasm32")]
fn load_token(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(key)
        .ok()?
}
#[cfg(target_arch = "wasm32")]
fn save_token(key: &str, token: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, token);
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn token_path(key: &str) -> Option<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".local/share"))
        })?;
    Some(
        root.join("nolan-chess/sessions")
            .join(format!("{:x}", Sha256::digest(key.as_bytes()))),
    )
}
#[cfg(not(target_arch = "wasm32"))]
fn load_token(key: &str) -> Option<String> {
    std::fs::read_to_string(token_path(key)?).ok()
}
#[cfg(not(target_arch = "wasm32"))]
fn save_token(key: &str, token: &str) {
    use std::io::Write;
    let Some(path) = token_path(key) else {
        return;
    };
    if std::fs::create_dir_all(path.parent().expect("token directory")).is_err() {
        return;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(path) {
        let _ = file.write_all(token.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opening() -> CommandProposal {
        CommandProposal {
            id: "a".repeat(32),
            revision: 0,
            side: Side::White,
            command: "move 0 12:0:0 e2 e4 + normal".into(),
        }
    }

    fn online_after_opening(side: Color) -> (GameUi, MultiplayerClient) {
        let mut game = GameUi::default();
        let prepared =
            validate_proposal(game.engine.clone(), game.record.clone(), &opening(), 0).unwrap();
        game.engine = prepared.engine;
        game.record = prepared.record;
        game.online.enabled = true;
        game.online.ready = true;
        game.online.opponent = true;
        game.online.side = Some(side);
        game.online.revision = 1;
        (
            game,
            MultiplayerClient {
                received_revision: 1,
                ..Default::default()
            },
        )
    }

    fn undo_request() -> UndoProposal {
        UndoProposal {
            id: "c".repeat(32),
            revision: 1,
            side: Side::White,
            keep_commands: 0,
        }
    }

    fn finish_network_replay(game: &mut GameUi) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while game.replay_job.is_some() {
            game.poll_replay();
            assert!(
                std::time::Instant::now() < deadline,
                "network replay timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!game.online.fatal, "{}", game.online.status.text());
    }

    #[test]
    fn online_undo_requests_consent_without_changing_the_position() {
        for side in [Color::White, Color::Black] {
            let (mut game, _) = online_after_opening(side);
            let before = game.engine.clone();
            assert!(game.can_undo(), "either player may request undo");
            game.undo_last_move();
            assert_eq!(game.engine, before);
            assert_eq!(game.record.commands().len(), 1);
            assert!(game.replay_job.is_none());
            assert!(matches!(
                game.online.action,
                Some(Action::RequestUndo { keep_commands: 0 })
            ));
            assert!(!game.can_undo());
            assert!(!game.can_submit());
        }
    }

    #[test]
    fn undo_is_not_an_automatic_command_vote_and_decline_preserves_the_game() {
        let (mut game, mut client) = online_after_opening(Color::Black);
        let before = game.engine.clone();
        let request = undo_request();
        receive_undo(&mut client, &mut game, request.clone());
        client.poll_validation(&mut game);
        assert!(client.validation.is_none());
        assert!(
            game.online.action.is_none(),
            "consent requires a user action"
        );
        assert!(!game.can_submit());
        game.online.answer_undo(false);
        assert!(matches!(
            game.online.action,
            Some(Action::UndoVote {
                accepted: false,
                ..
            })
        ));
        receive_undo(&mut client, &mut game, request.clone());
        assert!(
            game.online.undo_answered,
            "a retransmit must not reopen voting"
        );
        receive(
            &mut client,
            &mut game,
            ServerMessage::UndoDeclined {
                proposal_id: request.id,
            },
        );
        assert_eq!(game.engine, before);
        assert!(game.online.pending_undo.is_none());
        assert!(!game.online.waiting);
        assert!(game.can_submit());

        let (mut requester, mut client) = online_after_opening(Color::White);
        let request = undo_request();
        receive_undo(&mut client, &mut requester, request.clone());
        requester.online.answer_undo(true);
        assert!(
            requester.online.action.is_none(),
            "requesters cannot approve their own requests"
        );
        receive(
            &mut client,
            &mut requester,
            ServerMessage::UndoDeclined {
                proposal_id: request.id,
            },
        );
        assert!(requester.online.undo_notice.is_some());
    }

    #[test]
    fn undo_replays_both_clients_and_subsequent_moves_use_network_revision() {
        let mut clients = [
            online_after_opening(Color::White),
            online_after_opening(Color::Black),
        ];
        for (game, client) in &mut clients {
            let request = undo_request();
            receive_undo(client, game, request.clone());
            let message = ServerMessage::UndoAccepted {
                proposal_id: request.id,
                revision: 2,
                history: vec![],
            };
            receive(client, game, message.clone());
            receive(client, game, message); // Duplicate must not restart or repeat undo.
            assert!(game.online.synchronizing);
            assert_eq!(game.online.revision, 2);
            finish_network_replay(game);
            assert!(!game.online.synchronizing);
            assert!(game.record.commands().is_empty());
            assert!(game.online.pending_undo.is_none());
            assert_eq!(game.engine.turn(), Color::White);
            let mut next = opening();
            next.revision = 2;
            let prepared = validate_proposal(
                game.engine.clone(),
                game.record.clone(),
                &next,
                game.online.revision,
            )
            .unwrap();
            assert_eq!(prepared.record.commands().len(), 1);
        }
        assert_eq!(clients[0].0.engine, clients[1].0.engine);
        assert_eq!(clients[0].0.record, clients[1].0.record);
    }

    #[test]
    fn reconnect_restores_undo_and_expired_requests_clear_on_sync() {
        let (mut game, mut client) = online_after_opening(Color::Black);
        let request = undo_request();
        let history = vec![opening().command];
        snapshot(
            &mut client,
            &mut game,
            7,
            history.clone(),
            None,
            Some(UndoProposal {
                revision: 7,
                ..request.clone()
            }),
        );
        finish_network_replay(&mut game);
        assert!(game.online.pending_undo.is_some());
        assert!(game.online.waiting);
        assert_eq!(undo_prefix(&game.record), Some(0));
        snapshot(&mut client, &mut game, 7, history, None, None);
        finish_network_replay(&mut game);
        assert!(game.online.pending_undo.is_none());
        assert!(!game.online.waiting);
        assert!(game.can_submit());
    }

    #[test]
    fn lab_network_snapshot_and_consensual_undo_keep_the_prepared_history() {
        use temporal_chess_engine::{
            engine::LabEdit,
            model::{PieceKind, Sq},
        };
        let mut engine = Engine::new_lab(GameRules::default())
            .unwrap()
            .with_interactive_calculation()
            .unwrap();
        engine
            .edit_lab(LabEdit::Boards {
                beginning: -2,
                horizon: 6,
            })
            .unwrap();
        for (color, t, square) in [(Color::White, 0, Sq(1, 0)), (Color::Black, 1, Sq(1, 7))] {
            engine
                .edit_lab(LabEdit::Place {
                    t,
                    square,
                    color,
                    kind: PieceKind::N,
                    inverted: false,
                })
                .unwrap();
        }
        let preparation = CommandProposal {
            id: "e".repeat(32),
            revision: 0,
            side: Side::White,
            command: "move 0 0:0:0 b1 c3 + normal".into(),
        };
        let prepared = validate_proposal(
            engine.clone(),
            GameRecord::from_lab(&engine),
            &preparation,
            0,
        )
        .unwrap();
        let initial = prepared.engine;
        let seed = GameRecord::from_lab(&initial);
        let mut game = GameUi {
            engine: initial.clone(),
            record: seed.clone(),
            ..Default::default()
        };
        game.online.enabled = true;
        game.online.ready = true;
        game.online.side = Some(Color::Black);
        game.online.starting_position = StartingPosition::Lab;
        game.online.lab_setup = seed.lab_setup().cloned();
        let command = "move 1 1:1:0 b8 c6 + normal".to_owned();
        let mut client = MultiplayerClient::default();
        snapshot(&mut client, &mut game, 1, vec![command], None, None);
        finish_network_replay(&mut game);
        assert_eq!(game.record.commands().len(), 1);
        assert_eq!(game.engine.history().moves().len(), 2);
        let request = undo_request();
        receive_undo(&mut client, &mut game, request.clone());
        receive(
            &mut client,
            &mut game,
            ServerMessage::UndoAccepted {
                proposal_id: request.id,
                revision: 2,
                history: vec![],
            },
        );
        finish_network_replay(&mut game);
        assert_eq!(game.engine, initial);
        assert_eq!(game.record, seed);
    }

    #[test]
    fn consensus_validation_is_isolated_and_prepared_state_matches_replay() {
        let mut game = GameUi::default();
        let initial = game.record.to_text();
        let proposal = opening();
        let prepared =
            validate_proposal(game.engine.clone(), game.record.clone(), &proposal, 0).unwrap();
        assert_eq!(game.record.to_text(), initial);
        assert_eq!(game.engine.history().horizon(), 0);
        assert_eq!(game.engine.turn(), Color::White);
        assert_eq!(prepared.record.commands().len(), 1);
        let second_client =
            validate_proposal(game.engine.clone(), game.record.clone(), &proposal, 0).unwrap();
        assert_eq!(prepared.record.to_text(), second_client.record.to_text());
        let (replayed, _) = GameRecord::load(&prepared.record.to_text()).unwrap();
        assert_eq!(
            format!("{:?}", prepared.engine.history()),
            format!("{:?}", replayed.history())
        );
        game.apply_prepared(prepared.command, prepared.engine, prepared.record);
        assert_eq!(game.engine.turn(), Color::Black);
        assert_eq!(game.record.commands().len(), 1);
    }

    #[test]
    fn consensus_accepts_spatial_expansion_and_moves_from_negative_boards() {
        let mut engine = Engine::default().with_interactive_calculation().unwrap();
        let mut record = GameRecord::default();
        for (revision, side, command) in [
            (0, Side::White, "move 0 6:0:0 g1 f3 - normal"),
            (1, Side::Black, "move 1 17:1:0 b8 c6 + normal"),
            (2, Side::White, "move -2 6:0:1 f3 h4 - normal"),
        ] {
            let proposal = CommandProposal {
                id: "b".repeat(32),
                revision,
                side,
                command: command.into(),
            };
            let prepared = validate_proposal(engine, record, &proposal, revision).unwrap();
            engine = prepared.engine;
            record = prepared.record;
            assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
        }
        assert_eq!(engine.history().beginning(), -4);
    }

    #[test]
    fn clients_reject_wrong_actor_revision_and_illegal_commands() {
        let game = GameUi::default();
        let mut wrong_actor = opening();
        wrong_actor.side = Side::Black;
        let mut wrong_revision = opening();
        wrong_revision.revision = 1;
        let mut illegal = opening();
        illegal.command = "move 0 12:0:0 e2 e5 + normal".into();
        let mut malformed = opening();
        malformed.command = "opaque command".into();
        for proposal in [wrong_actor, wrong_revision, illegal, malformed] {
            assert!(
                validate_proposal(game.engine.clone(), game.record.clone(), &proposal, 0).is_err()
            );
        }
        assert!(game.record.commands().is_empty());
    }

    #[test]
    fn lab_moves_with_retained_knowledge_of_trimmed_future_are_accepted() {
        use temporal_chess_engine::{
            engine::LabEdit,
            model::{PieceKind, Sq},
        };
        let mut engine = Engine::new_lab(Default::default())
            .unwrap()
            .with_interactive_calculation()
            .unwrap();
        engine
            .edit_lab(LabEdit::Boards {
                beginning: -3,
                horizon: 1,
            })
            .unwrap();
        engine
            .edit_lab(LabEdit::Place {
                t: 1,
                square: Sq(1, 7),
                kind: PieceKind::N,
                color: Color::Black,
                inverted: true,
            })
            .unwrap();
        engine.edit_lab(LabEdit::Turn(Color::Black)).unwrap();
        let piece = engine.move_sources().next().unwrap();
        let mv = engine
            .possible_moves(0, piece.age)
            .unwrap()
            .into_iter()
            .find(|m| m.to_past() && m.sq == Sq(2, 5))
            .unwrap();
        engine.apply_new_mv(mv).unwrap();
        engine
            .edit_lab(LabEdit::Boards {
                beginning: -3,
                horizon: 3,
            })
            .unwrap();
        engine.edit_lab(LabEdit::RemoveBoard { t: 3 }).unwrap();
        engine.edit_lab(LabEdit::Turn(Color::Black)).unwrap();
        let proposal = CommandProposal {
            id: "f".repeat(32),
            revision: 17,
            side: Side::Black,
            command: "move -1 0:3:0 c6 e5 + normal".into(),
        };
        let prepared =
            validate_proposal(engine.clone(), GameRecord::from_lab(&engine), &proposal, 17)
                .unwrap();
        assert_eq!(
            GameRecord::load(&prepared.record.to_text()).unwrap().0,
            prepared.engine
        );
    }

    #[test]
    fn proposal_and_rejection_do_not_change_the_live_game() {
        let mut game = GameUi::default();
        let mut client = MultiplayerClient::default();
        let proposal = opening();
        receive_proposal(&mut client, &mut game, proposal.clone());
        let prepared =
            validate_proposal(game.engine.clone(), game.record.clone(), &proposal, 0).unwrap();
        client.voted = Some(proposal.id.clone());
        client.prepared = Some((proposal.clone(), prepared));
        receive_proposal(&mut client, &mut game, proposal.clone());
        assert!(client.prepared.is_some());
        assert_eq!(client.voted.as_ref(), Some(&proposal.id));
        assert!(game.record.commands().is_empty());
        receive(
            &mut client,
            &mut game,
            ServerMessage::CommandRejected {
                proposal_id: proposal.id,
                side: Side::Black,
                reason: "test rejection".into(),
            },
        );
        assert!(client.proposal.is_none());
        assert!(client.prepared.is_none());
        assert!(client.voted.is_none());
        assert!(!game.online.waiting);
        assert!(game.record.commands().is_empty());
    }

    #[test]
    fn acceptance_keeps_prepared_result_until_animation_can_start() {
        let mut game = GameUi::default();
        let mut client = MultiplayerClient::default();
        let proposal = opening();
        receive_proposal(&mut client, &mut game, proposal.clone());
        client.prepared = Some((
            proposal.clone(),
            validate_proposal(game.engine.clone(), game.record.clone(), &proposal, 0).unwrap(),
        ));
        let accepted = ServerMessage::CommandAccepted {
            revision: 1,
            command: proposal.command.clone(),
        };
        receive(&mut client, &mut game, accepted.clone());
        receive(&mut client, &mut game, accepted);
        assert_eq!(client.accepted.len(), 1);
        assert!(client.proposal.is_none());
        assert!(client.prepared.is_some());
        assert!(game.record.commands().is_empty());
        let mut next = proposal;
        next.id = "b".repeat(32);
        next.revision = 1;
        next.side = Side::Black;
        next.command = "move 1 28:1:0 e7 e5 + normal".into();
        receive_proposal(&mut client, &mut game, next);
        assert!(
            client.prepared.is_some(),
            "next proposal must not discard the preceding committed result"
        );
        client.poll_validation(&mut game);
        assert!(
            client.validation.is_none(),
            "cannot validate against a position before the preceding commit"
        );
    }
}
