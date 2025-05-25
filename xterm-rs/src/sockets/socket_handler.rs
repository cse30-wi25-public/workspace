use crate::models::{AppState, logger};
use axum::{
    extract::{
        Extension,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use bytes::Bytes;
use std::sync::Arc;
use tokio::select;

use crate::models::ClientMsg;

pub async fn ws_handler(ws: WebSocketUpgrade, Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client_session(socket, state))
}

async fn client_session(mut socket: WebSocket, state: Arc<AppState>) {
    let (mut rx, history) = state.pty.subscribe().await;
    if let Err(e) = socket.send(Message::Binary(Bytes::from(history.to_vec()))).await {
        logger("error", format!("Failed to send history: {}", e));
        return;
    }

    let mut cfg_rx = state.watcher.subscribe();
    let cfg = state.watcher.current();
    let payload = serde_json::json!({
        "event": "config",
        "value": cfg
    });
    let _ = socket.send(Message::from(payload.to_string())).await;

    loop {
        select! {
            Ok(bytes) = rx.recv() => {
                socket.send(Message::Binary(Bytes::copy_from_slice(&bytes))).await.ok();
                if let Some(caster) = &state.caster {
                    caster.output(state.start.elapsed().as_secs_f32(), bytes.to_vec());
                }
            }

            Ok(()) = cfg_rx.changed() => {
                let cfg = cfg_rx.borrow().clone();
                let payload = serde_json::json!({
                    "event": "config",
                    "value": cfg
                });
                let _ = socket.send(Message::from(payload.to_string())).await;
            }

            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(txt))) => {
                        if let Ok(cmd) = serde_json::from_str::<ClientMsg>(&txt) {
                            if handle(cmd, &state, &mut socket).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bin))) => {
                        if let Ok(cmd) = serde_json::from_slice::<ClientMsg>(&bin) {
                            if handle(cmd, &state, &mut socket).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn handle(msg: ClientMsg, state: &AppState, sock: &mut WebSocket) -> anyhow::Result<()> {
    match msg {
        ClientMsg::Data { value } => {
            if let Some(caster) = &state.caster {
                caster.input(state.start.elapsed().as_secs_f32(), value.as_bytes().to_vec());
            }
            state.pty.write(value.as_bytes()).await?;
        }
        ClientMsg::Resize { value } => {
            if let Some(caster) = &state.caster {
                caster.resize(state.start.elapsed().as_secs_f32(), value.rows, value.cols);
            }
            state.pty.resize(value.rows, value.cols).await?;
            let mut sz = state.stty_size.write().await;
            *sz = (value.rows, value.cols);
        }
        ClientMsg::Heartbeat => {
            if let Some(caster) = &state.caster {
                caster.heartbeat();
            }
            sock.send(Message::Text(r#"{"event":"heartbeat-pong"}"#.into())).await?;
        }
    }
    Ok(())
}
