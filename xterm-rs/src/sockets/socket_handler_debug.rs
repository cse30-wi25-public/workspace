use crate::models::AppState;
use crate::models::ClientMsg;
use crate::pty::PtyManager;
use axum::{
    extract::{
        Extension,
        ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};

use bytes::Bytes;
use std::sync::Arc;

use tokio::select;

pub async fn ws_handler_debug(ws: WebSocketUpgrade, Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let size_lock = Arc::clone(&state.stty_size);

    ws.on_upgrade(move |mut socket| async move {
        let (rows, cols) = *size_lock.read().await;
        match PtyManager::new(rows, cols, 0).await {
            Ok(new_pty) => {
                let pty = Arc::new(new_pty);
                debug_session(socket, pty).await;
            }
            Err(e) => {
                let _ = socket
                    .send(Message::Close(Some(CloseFrame {
                        code: 1011,
                        reason: Utf8Bytes::from(format!("pty init error: {e}")),
                    })))
                    .await;
            }
        }
    })
}

async fn debug_session(mut socket: WebSocket, pty: Arc<PtyManager>) {
    let (mut rx, history) = pty.subscribe().await;
    let _ = socket.send(Message::Binary(Bytes::from(history.to_vec()))).await;

    loop {
        select! {
            Ok(bytes) = rx.recv() => {
                socket.send(Message::Binary(Bytes::from(bytes))).await.ok();
            }

            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(txt))) => {
                    if let Ok(cmd) = serde_json::from_str::<ClientMsg>(&txt) {
                        apply_cmd(cmd, &pty).await;
                    }
                }
                Some(Ok(Message::Binary(bin))) => {
                    if let Ok(cmd) = serde_json::from_slice::<ClientMsg>(&bin) {
                        apply_cmd(cmd, &pty).await;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    }
}

async fn apply_cmd(cmd: ClientMsg, pty: &PtyManager) {
    match cmd {
        ClientMsg::Data { value } => {
            let _ = pty.write(value.as_bytes()).await;
        }
        ClientMsg::Resize { value } => {
            let _ = pty.resize(value.rows, value.cols).await;
        }
        _ => {}
    }
}
