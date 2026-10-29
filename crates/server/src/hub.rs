use std::{collections::HashMap, sync::Arc};

use temporal_chess_protocol::{ServerMessage, Side};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(crate) enum Outbound {
    Message(ServerMessage),
    Close,
}

#[derive(Clone)]
struct PlayerConnection {
    connection_id: Uuid,
    sender: mpsc::Sender<Outbound>,
}

#[derive(Clone, Default)]
pub(crate) struct ConnectionHub {
    rooms: Arc<RwLock<HashMap<String, HashMap<Side, PlayerConnection>>>>,
}

impl ConnectionHub {
    pub(crate) async fn is_current(&self, game_id: &str, side: Side, id: Uuid) -> bool {
        self.rooms
            .read()
            .await
            .get(game_id)
            .and_then(|room| room.get(&side))
            .is_some_and(|connection| connection.connection_id == id)
    }

    pub(crate) async fn register(
        &self,
        game_id: &str,
        side: Side,
        connection_id: Uuid,
        sender: mpsc::Sender<Outbound>,
    ) -> bool {
        let mut rooms = self.rooms.write().await;
        let room = rooms.entry(game_id.to_owned()).or_default();
        let opponent_connected = room.contains_key(&side.opposite());
        if let Some(previous) = room.insert(
            side,
            PlayerConnection {
                connection_id,
                sender,
            },
        ) {
            let _ = previous
                .sender
                .try_send(Outbound::Message(ServerMessage::Error {
                    code: "connection_replaced".to_owned(),
                    message: "This player reconnected from another client.".to_owned(),
                }));
            let _ = previous.sender.try_send(Outbound::Close);
        }
        opponent_connected
    }

    pub(crate) async fn unregister(&self, game_id: &str, side: Side, connection_id: Uuid) {
        let mut rooms = self.rooms.write().await;
        let mut remove_room = false;
        if let Some(room) = rooms.get_mut(game_id) {
            if room
                .get(&side)
                .is_some_and(|connection| connection.connection_id == connection_id)
            {
                room.remove(&side);
                if let Some(opponent) = room.get(&side.opposite()) {
                    let _ = opponent.sender.try_send(Outbound::Message(
                        ServerMessage::OpponentConnection { connected: false },
                    ));
                }
            }
            remove_room = room.is_empty();
        }
        if remove_room {
            rooms.remove(game_id);
        }
    }

    pub(crate) async fn notify_opponent(&self, game_id: &str, side: Side, message: ServerMessage) {
        let rooms = self.rooms.read().await;
        if let Some(opponent) = rooms
            .get(game_id)
            .and_then(|room| room.get(&side.opposite()))
        {
            let _ = opponent.sender.try_send(Outbound::Message(message));
        }
    }

    pub(crate) async fn broadcast(&self, game_id: &str, message: ServerMessage) {
        let rooms = self.rooms.read().await;
        if let Some(room) = rooms.get(game_id) {
            for connection in room.values() {
                let _ = connection
                    .sender
                    .try_send(Outbound::Message(message.clone()));
            }
        }
    }
}
