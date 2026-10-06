use crate::dashboard::DashboardServer;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

const CLIENT_QUEUE_CAPACITY: usize = 64;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<DashboardServer>>,
) -> Response {
    let event_rx = server.event_tx.subscribe();
    ws.on_upgrade(move |socket| handle_ws(socket, event_rx))
}

async fn handle_ws(socket: WebSocket, mut event_rx: broadcast::Receiver<String>) {
    let (sender, mut receiver) = socket.split();
    let (tx, rx) = mpsc::channel::<String>(CLIENT_QUEUE_CAPACITY);

    // Pump daemon events into the client's bounded queue. When the client is
    // too slow, frames are dropped here — daemon throughput is never affected.
    let forwarder = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(msg) => {
                    if tx.try_send(msg).is_err() {
                        debug!("websocket client queue full, dropping event");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("websocket event receiver lagged by {n} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let writer = tokio::spawn(async move {
        let mut sender = sender;
        let mut rx = rx;
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Read until the client closes or errors. Pings are answered
    // automatically by the underlying tungstenite stream.
    while let Some(Ok(_)) = receiver.next().await {}

    forwarder.abort();
    writer.abort();
}
