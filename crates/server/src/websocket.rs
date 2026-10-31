use actix_web::{HttpRequest, HttpResponse, web};
use actix_ws::{AggregatedMessage, Session};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use temporal_chess_protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, SyncReason};
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::{
    database::{AuthenticatedPlayer, CommandResult, Repository, RepositoryError},
    hub::{ConnectionHub, Outbound},
};

const MAX_FRAME_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) repository: Repository,
    pub(crate) hub: ConnectionHub,
    pub(crate) ordering: RoomOrdering,
}

/// Serialize snapshot/register and commit/broadcast per room, not just SQL writes.
/// Weak entries are pruned so completed rooms do not accumulate mutexes forever.
#[derive(Clone, Default)]
pub(crate) struct RoomOrdering(Arc<Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>>);
impl RoomOrdering {
    async fn lock(&self, room: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let mutex = {
            let mut rooms = self.0.lock().await;
            rooms.retain(|_, weak| weak.strong_count() > 0);
            match rooms.get(room).and_then(std::sync::Weak::upgrade) {
                Some(mutex) => mutex,
                None => {
                    let mutex = Arc::new(Mutex::new(()));
                    rooms.insert(room.to_owned(), Arc::downgrade(&mutex));
                    mutex
                }
            }
        };
        mutex.lock_owned().await
    }
}

pub(crate) async fn websocket(
    request: HttpRequest,
    body: web::Payload,
    state: web::Data<AppState>,
) -> actix_web::Result<HttpResponse> {
    let (response, session, stream) = actix_ws::handle(&request, body)?;
    actix_web::rt::spawn(connection_task(
        session,
        stream
            .max_frame_size(MAX_FRAME_BYTES)
            .aggregate_continuations()
            .max_continuation_size(MAX_FRAME_BYTES),
        state.get_ref().clone(),
    ));
    Ok(response)
}

async fn connection_task(
    mut session: Session,
    mut stream: actix_ws::AggregatedMessageStream,
    state: AppState,
) {
    let connection_id = Uuid::new_v4();
    let (outbound_sender, mut outbound_receiver) = mpsc::channel(64);
    let mut authenticated: Option<AuthenticatedPlayer> = None;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    let mut last_seen = Instant::now();
    let mut rate_window = Instant::now();
    let mut messages = 0u32;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if last_seen.elapsed() > Duration::from_secs(60) { break; }
                if session.ping(b"heartbeat").await.is_err() { break; }
            },
            outbound = outbound_receiver.recv() => match outbound {
                Some(Outbound::Message(message)) => {
                    if !matches!(tokio::time::timeout(Duration::from_secs(10), send_message(&mut session, &message)).await, Ok(Ok(()))) {
                        break;
                    }
                }
                Some(Outbound::Close) | None => break,
            },
            incoming = stream.recv() => match incoming {
                Some(Ok(AggregatedMessage::Text(text))) => {
                    last_seen = Instant::now();
                    if rate_window.elapsed() >= Duration::from_secs(1) {
                        rate_window = Instant::now();
                        messages = 0;
                    }
                    messages += 1;
                    if messages > 20 { break; }
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(message) => handle_client_message(
                            &state,
                            connection_id,
                            &outbound_sender,
                            &mut authenticated,
                            message,
                        ).await,
                        Err(_) => send_error(
                            &outbound_sender,
                            "invalid_json",
                            "The WebSocket message is not valid protocol JSON.",
                        ),
                    }
                }
                Some(Ok(AggregatedMessage::Ping(bytes))) => {
                    if session.pong(&bytes).await.is_err() {
                        break;
                    }
                }
                Some(Ok(AggregatedMessage::Pong(_))) => { last_seen = Instant::now(); }
                Some(Ok(AggregatedMessage::Close(_))) | None => break,
                Some(Ok(AggregatedMessage::Binary(_))) => send_error(
                    &outbound_sender,
                    "binary_unsupported",
                    "Only JSON text messages are supported.",
                ),
                Some(Err(error)) => {
                    tracing::debug!(%connection_id, %error, "websocket protocol error");
                    break;
                }
            }
        }
    }

    if let Some(auth) = authenticated {
        let _ordering = state.ordering.lock(&auth.game_id).await;
        state
            .hub
            .unregister(&auth.game_id, auth.side, connection_id)
            .await;
    }
    let _ = session.close(None).await;
}

async fn handle_client_message(
    state: &AppState,
    connection_id: Uuid,
    sender: &mpsc::Sender<Outbound>,
    authenticated: &mut Option<AuthenticatedPlayer>,
    message: ClientMessage,
) {
    let key = authenticated
        .as_ref()
        .map(|auth| auth.game_id.clone())
        .unwrap_or_else(|| match &message {
            ClientMessage::JoinGame { game_id, .. } => game_id.trim().to_ascii_uppercase(),
            _ => String::new(),
        });
    let _ordering = state.ordering.lock(&key).await;
    if let Some(auth) = authenticated.as_ref()
        && !state
            .hub
            .is_current(&auth.game_id, auth.side, connection_id)
            .await
    {
        send_error(
            sender,
            "connection_replaced",
            "Подключение заменено другим клиентом.",
        );
        return;
    }
    match message {
        ClientMessage::CreateGame {
            protocol,
            side,
            causal_gaps,
        } => {
            if !check_handshake(protocol, authenticated, sender) {
                return;
            }
            match state.repository.create_game(side, causal_gaps).await {
                Ok(ready) => {
                    let opponent_connected = state
                        .hub
                        .register(
                            &ready.auth.game_id,
                            ready.auth.side,
                            connection_id,
                            sender.clone(),
                        )
                        .await;
                    send_local(
                        sender,
                        ServerMessage::GameReady {
                            causal_gaps: ready.causal_gaps,
                            created: true,
                            game_id: ready.auth.game_id.clone(),
                            player_token: ready.auth.player_token.clone(),
                            side: ready.auth.side,
                            revision: ready.revision,
                            history: ready.history,
                            opponent_connected,
                        },
                    );
                    *authenticated = Some(ready.auth);
                }
                Err(error) => send_repository_error(sender, error),
            }
        }
        ClientMessage::JoinGame {
            protocol,
            game_id,
            player_token,
        } => {
            if !check_handshake(protocol, authenticated, sender) {
                return;
            }
            match state
                .repository
                .join_game(&game_id, player_token.as_deref())
                .await
            {
                Ok(ready) => {
                    let opponent_connected = state
                        .hub
                        .register(
                            &ready.auth.game_id,
                            ready.auth.side,
                            connection_id,
                            sender.clone(),
                        )
                        .await;
                    state
                        .hub
                        .notify_opponent(
                            &ready.auth.game_id,
                            ready.auth.side,
                            ServerMessage::OpponentConnection { connected: true },
                        )
                        .await;
                    send_local(
                        sender,
                        ServerMessage::GameReady {
                            causal_gaps: ready.causal_gaps,
                            created: false,
                            game_id: ready.auth.game_id.clone(),
                            player_token: ready.auth.player_token.clone(),
                            side: ready.auth.side,
                            revision: ready.revision,
                            history: ready.history,
                            opponent_connected,
                        },
                    );
                    *authenticated = Some(ready.auth);
                }
                Err(error) => send_repository_error(sender, error),
            }
        }
        ClientMessage::PlayCommand { revision, command } => {
            let Some(auth) = authenticated.as_ref() else {
                send_error(sender, "not_authenticated", "Create or join a game first.");
                return;
            };
            match state
                .repository
                .play_command(auth, revision, &command)
                .await
            {
                Ok(CommandResult::Accepted { revision, command }) => {
                    state
                        .hub
                        .broadcast(
                            &auth.game_id,
                            ServerMessage::CommandAccepted { revision, command },
                        )
                        .await;
                }
                Ok(CommandResult::Sync {
                    revision,
                    history,
                    reason,
                }) => send_local(
                    sender,
                    ServerMessage::Sync {
                        revision,
                        history,
                        reason,
                    },
                ),
                Err(error) => send_repository_error(sender, error),
            }
        }
        ClientMessage::Resync { revision: _ } => {
            let Some(auth) = authenticated.as_ref() else {
                send_error(sender, "not_authenticated", "Create or join a game first.");
                return;
            };
            match state.repository.resync(auth).await {
                Ok((revision, history)) => send_local(
                    sender,
                    ServerMessage::Sync {
                        revision,
                        history,
                        reason: SyncReason::Requested,
                    },
                ),
                Err(error) => send_repository_error(sender, error),
            }
        }
    }
}

fn check_handshake(
    protocol: u16,
    authenticated: &Option<AuthenticatedPlayer>,
    sender: &mpsc::Sender<Outbound>,
) -> bool {
    if protocol != PROTOCOL_VERSION {
        send_error(
            sender,
            "protocol_mismatch",
            "Client and server protocol versions do not match.",
        );
        return false;
    }
    if authenticated.is_some() {
        send_error(
            sender,
            "already_authenticated",
            "This WebSocket already belongs to a game.",
        );
        return false;
    }
    true
}

fn send_repository_error(sender: &mpsc::Sender<Outbound>, error: RepositoryError) {
    match error {
        RepositoryError::Client { code, message } => send_error(sender, code, &message),
        internal => {
            tracing::error!(error = %internal, "multiplayer repository failure");
            send_error(
                sender,
                "server_error",
                "The server could not process the request.",
            );
        }
    }
}

fn send_error(sender: &mpsc::Sender<Outbound>, code: &str, message: &str) {
    send_local(
        sender,
        ServerMessage::Error {
            code: code.to_owned(),
            message: message.to_owned(),
        },
    );
}

fn send_local(sender: &mpsc::Sender<Outbound>, message: ServerMessage) {
    let _ = sender.try_send(Outbound::Message(message));
}

async fn send_message(
    session: &mut Session,
    message: &ServerMessage,
) -> Result<(), actix_ws::Closed> {
    let json = serde_json::to_string(message).expect("server protocol messages are serializable");
    session.text(json).await
}
