//! Room transport is non-Send (browser sockets), separate from the game resource.
//! Revisions count journal commands, including free temporal advances/calculation.
use crate::controller::{GameUi, side_name};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};
use temporal_chess_engine::{model::Color, record::Command};
use temporal_chess_protocol::{
    ClientMessage, MAX_COMMAND_LEN, MAX_COMMANDS, PROTOCOL_VERSION, ServerMessage, Side,
    SidePreference,
};
use web_time::Instant;

pub struct OnlineState {
    pub enabled: bool,
    pub ready: bool,
    pub waiting: bool,
    pub synchronizing: bool,
    pub fatal: bool,
    pub side: Option<Color>,
    pub revision: u64,
    pub outgoing: Option<Command>,
    pub status: String,
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
            enabled: false,
            ready: false,
            waiting: false,
            synchronizing: false,
            fatal: false,
            side: None,
            revision: 0,
            outgoing: None,
            status: String::new(),
            endpoint: default_endpoint(),
            room_input: invite_room(),
            room: None,
            preference: SidePreference::Random,
            opponent: false,
            action: None,
        }
    }
}
enum Action {
    Create,
    Join,
    Retry,
    Leave,
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
}
impl MultiplayerClient {
    fn close(&mut self) {
        if let Some(mut sender) = self.sender.take() {
            sender.close();
        }
        self.receiver = None;
        self.handshake = None;
        self.deadline = None;
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
                max_incoming_frame_size: 4 * 1024 * 1024,
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
        game.online.outgoing = None;
        game.pending_move = None;
        game.pending_square = None;
        game.clear_selection();
        if game.online.fatal {
            return;
        }
        if game.online.room.is_some() && self.token.is_some() {
            self.retry_delay = (self.retry_delay.max(1) * 2).min(20);
            self.retry_at = Some(Instant::now() + Duration::from_secs(self.retry_delay));
            game.online.status = "Связь потеряна. Переподключение…".into();
        } else {
            game.online.fatal = true;
            game.online.status = "Не удалось подключиться. Проверьте адрес сервера.".into();
        }
    }
    fn resync(&mut self, game: &mut GameUi) {
        game.online.synchronizing = true;
        game.online.waiting = true;
        game.online.outgoing = None;
        self.accepted.clear();
        self.awaiting_snapshot = true;
        self.send(&ClientMessage::Resync {
            revision: game.online.revision,
        });
        self.deadline = Some(Instant::now() + Duration::from_secs(30));
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
            Action::Leave => {
                client.close();
                client.retry_at = None;
                client.accepted.clear();
                game.replay_job = None;
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
                game.online.status = "Подключение…".into();
                game.pending_square = None;
                game.pending_move = None;
                game.clear_selection();
                if let Err(error) = client.connect(&game.online.endpoint, handshake, ctx) {
                    client.disconnected(&mut game);
                    if game.online.fatal {
                        game.online.status = error;
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
                        game.online.status =
                            "Неподдерживаемый ответ сервера. Обновите клиент.".into();
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
    if game.transition.is_none() {
        if let Some((revision, text)) = client.accepted.pop_front() {
            if revision != game.online.revision + 1 {
                client.resync(&mut game);
                return;
            }
            let result = text
                .parse::<Command>()
                .and_then(|command| game.apply_confirmed(command));
            if result.is_err() {
                // A different engine build must not loop on the same invalid journal.
                client.resync(&mut game);
                return;
            }
            game.online.revision = revision;
            if client.accepted.is_empty() {
                game.online.waiting = false;
            }
        }
    }
    if let Some(command) = game.online.outgoing.take() {
        let revision = game.online.revision;
        client.send(&ClientMessage::PlayCommand {
            revision,
            command: command.to_string(),
        });
        client.deadline = Some(Instant::now() + Duration::from_secs(30));
    }
}

fn receive(client: &mut MultiplayerClient, game: &mut GameUi, message: ServerMessage) {
    match message {
        ServerMessage::GameReady {
            game_id,
            player_token,
            side,
            revision,
            history,
            opponent_connected,
            ..
        } => {
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
            snapshot(client, game, revision, history);
        }
        ServerMessage::Sync {
            revision, history, ..
        } => snapshot(client, game, revision, history),
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
            client.received_revision = revision;
            client.accepted.push_back((revision, command));
            game.online.waiting = true;
            client.deadline = None;
            game.clear_selection();
        }
        ServerMessage::OpponentConnection { connected } => game.online.opponent = connected,
        ServerMessage::Error { code, message } => {
            game.online.outgoing = None;
            game.online.waiting = !client.accepted.is_empty();
            client.deadline = None;
            game.online.status = message;
            if !game.online.ready
                || matches!(
                    code.as_str(),
                    "connection_replaced"
                        | "invalid_player_token"
                        | "protocol_mismatch"
                        | "game_not_found"
                        | "game_full"
                )
            {
                game.online.fatal = true;
                game.online.ready = false;
            }
        }
    }
}

fn snapshot(
    client: &mut MultiplayerClient,
    game: &mut GameUi,
    revision: u64,
    history: Vec<String>,
) {
    if revision != history.len() as u64
        || history.len() > MAX_COMMANDS
        || history
            .iter()
            .any(|line| line.len() > MAX_COMMAND_LEN || line.contains(['\r', '\n']))
    {
        game.online.fatal = true;
        game.online.ready = false;
        game.online.status = "Некорректный журнал сервера.".into();
        return;
    }
    client.deadline = None;
    client.accepted.clear();
    client.received_revision = revision;
    client.awaiting_snapshot = false;
    game.online.revision = revision;
    game.online.waiting = false;
    game.online.outgoing = None;
    game.online.synchronizing = true;
    game.begin_network_replay(&history);
}

pub fn panel(ui: &mut egui::Ui, game: &mut GameUi) {
    let idle = game.replay_job.is_none() && game.transition.is_none();
    ui.add_space(8.0);
    if game.online.enabled {
        ui.strong("Игра по сети");
        if let Some(room) = &game.online.room {
            ui.horizontal(|ui| {
                ui.monospace(room);
                if ui.small_button("Копировать код").clicked() {
                    ui.ctx().copy_text(room.clone());
                }
            });
        }
        if let Some(side) = game.online.side {
            ui.label(format!("Вы играете: {}", side_name(side)));
        }
        if !game.online.status.is_empty() {
            ui.label(&game.online.status);
        }
        if game.online.ready {
            ui.label(if game.online.synchronizing {
                "Восстановление партии…"
            } else if game.online.waiting {
                "Ожидание подтверждения…"
            } else if game.online.opponent {
                "Соперник подключён"
            } else {
                "Ожидание соперника"
            });
        }
        ui.horizontal(|ui| {
            if game.online.fatal && ui.button("Подключиться снова").clicked() {
                game.online.action = Some(Action::Retry);
            }
            if ui.button("Выйти в локальный режим").clicked() {
                game.online.action = Some(Action::Leave);
            }
        });
        ui.small("Отмена и импорт доступны в локальном режиме.");
    } else {
        egui::CollapsingHeader::new("Играть по сети")
            .default_open(!game.online.room_input.is_empty())
            .show(ui, |ui| {
                ui.add_enabled_ui(idle, |ui| {
                    ui.label("Создать новую комнату");
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut game.online.preference,
                            SidePreference::Random,
                            "Случайно",
                        );
                        ui.selectable_value(
                            &mut game.online.preference,
                            SidePreference::White,
                            "Белые",
                        );
                        ui.selectable_value(
                            &mut game.online.preference,
                            SidePreference::Black,
                            "Чёрные",
                        );
                    });
                    if ui.button("Создать комнату").clicked() {
                        game.online.action = Some(Action::Create);
                    }
                    ui.separator();
                    ui.add(
                        egui::TextEdit::singleline(&mut game.online.room_input)
                            .hint_text("Код комнаты")
                            .char_limit(12),
                    );
                    if ui
                        .add_enabled(
                            !game.online.room_input.trim().is_empty(),
                            egui::Button::new("Присоединиться"),
                        )
                        .clicked()
                    {
                        game.online.action = Some(Action::Join);
                    }
                    egui::CollapsingHeader::new("Сервер").show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut game.online.endpoint)
                                .hint_text("wss://example.com/ws"),
                        );
                    });
                    ui.small("При подключении текущую позицию заменит сетевая партия.");
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
