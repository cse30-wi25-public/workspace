use crate::caster::Caster;
use crate::config::ConfigWatcher;
use crate::pty::PtyManager;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use memchr::memrchr;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::{sync::Arc, time::Instant};
use tokio::sync::RwLock;
use unicode_width::UnicodeWidthChar;

// app config
fn default_layout() -> String {
    "qwerty".into()
}
fn default_theme() -> String {
    "Default".into()
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default = "default_theme")]
    pub theme: String,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum ClientMsg {
    Data { value: String },
    Resize { value: SttySize },
    Heartbeat,
}

#[derive(Deserialize, Debug)]
pub struct SttySize {
    pub cols: u16,
    pub rows: u16,
}

pub struct AppState {
    pub start: Instant,
    pub pty: Arc<PtyManager>,
    pub caster: Option<Arc<Caster>>,
    pub watcher: ConfigWatcher,
    pub stty_size: Arc<RwLock<(u16, u16)>>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match &self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()).into_response(),
        }
    }
}

pub fn buf_trim(buf: &[u8], cols: u16, max_lines: u32) -> usize {
    let mut lines = 0;
    let mut col = 0;
    let mut i = buf.len();

    while i > 0 && lines < max_lines {
        i -= 1;
        match buf[i] {
            b'\n' => {
                lines += 1;
                col = 0;
            }
            0x20..=0x7e => {
                col += 1;
                if col == cols {
                    lines += 1;
                    col = 0;
                }
            }
            0x80..=0xff => {
                let mut start = i;
                while start > 0 && (buf[start] & 0b1100_0000) == 0b1000_0000 {
                    start -= 1;
                }
                let ch = std::str::from_utf8(&buf[start..=i])
                    .ok()
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ');
                col += UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                if col >= cols {
                    lines += 1;
                    col = if col == cols { 0 } else { col - cols };
                }
                i = start;
            }
            0x1b => {
                if let Some(pos) = memrchr(b'm', &buf[..=i]) {
                    i = pos.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
    i
}

pub fn logger<P>(kind: &str, payload: P)
where
    P: Serialize,
{
    let payload = serde_json::json!([kind, payload]);
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &payload).ok();
    stdout.write_all(b"\n").ok();
    stdout.flush().ok();
}

// loop queue
#[derive(Clone)]
pub struct RingBytes {
    buf: VecDeque<u8>,
    limit: usize,
}

impl RingBytes {
    pub fn new(limit: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(limit),
            limit,
        }
    }

    pub fn extend(&mut self, chunk: &[u8]) {
        match chunk.len().checked_sub(self.limit) {
            Some(x) => {
                self.buf.clear();
                self.buf.extend(&chunk[x..]);
            }
            None => {
                if let Some(x) = (self.buf.len() + chunk.len()).checked_sub(self.limit) {
                    self.buf.drain(..x);
                }
                self.buf.extend(chunk);
            }
        }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let (a, b) = self.buf.as_slices();
        let mut v = Vec::with_capacity(self.buf.len());
        v.extend_from_slice(a);
        v.extend_from_slice(b);
        v
    }
}
