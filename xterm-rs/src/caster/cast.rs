use crate::models::{buf_trim, logger};
use base64::Engine as _;
use std::sync::Arc;
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::mpsc,
    time::{self, Duration},
};
use unsigned_varint::encode as varint;
use zstd::stream::encode_all;

const HEARTBEAT_FN: &str = "heartbeat.log";

#[derive(Clone, Copy, Debug)]
enum EventKind {
    Input,
    Output,
    Resize,
}

#[derive(Debug)]
pub struct RawEvt {
    elapsed: f32,
    kind: EventKind,
    payload: Vec<u8>,
}

fn encode_evt(e: &RawEvt) -> Vec<u8> {
    // estimate 4(elapsed)+1(kind)+5(varint)+payload
    let mut v = Vec::with_capacity(10 + e.payload.len());
    v.extend_from_slice(&e.elapsed.to_le_bytes());
    v.push(e.kind as u8);

    let mut len_buf = [0u8; 5];
    if matches!(e.kind, EventKind::Input | EventKind::Output) {
        let var = varint::u32(e.payload.len() as u32, &mut len_buf);
        v.extend_from_slice(var);
    }
    v.extend_from_slice(&e.payload);
    v
}

fn write_binary(file: &mut BufWriter<std::fs::File>, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes)?;
    file.flush()
}

pub struct Caster {
    cast_tx: mpsc::UnboundedSender<RawEvt>,
    hb_tx: mpsc::UnboundedSender<u32>,
}

impl Caster {
    pub fn new(
        log_dir: std::path::PathBuf,
        start: std::time::Instant,
        timestamp: u128,
        verbose_log: bool,
        verbose_interval: u32,
        stty_size: (u16, u16), // rows, cols
    ) -> anyhow::Result<Arc<Self>> {
        if log_dir.exists() && !log_dir.is_dir() {
            anyhow::bail!("'{}' exists and is not a directory", log_dir.display());
        }
        std::fs::create_dir_all(&log_dir)?;

        let cast_path = log_dir.join(format!("{}.cast", timestamp));
        let hb_path = log_dir.join(HEARTBEAT_FN);

        let cast_file = BufWriter::new(OpenOptions::new().create(true).append(true).open(&cast_path)?);
        let hb_file = BufWriter::new(OpenOptions::new().create(true).append(true).open(&hb_path)?);

        let (cast_tx, mut cast_rx) = mpsc::unbounded_channel::<RawEvt>();
        let (hb_tx, mut hb_rx) = mpsc::unbounded_channel::<u32>();

        tokio::spawn(async move {
            let mut cast_file = cast_file;
            let mut hb_file = hb_file;

            let mut buf_disk: Vec<u8> = Vec::new();
            let mut buf_stdout: Vec<u8> = Vec::new();

            let mut flush_disk = time::interval(Duration::from_millis(10));
            flush_disk.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

            let mut flush_stdout = time::interval(Duration::from_secs(verbose_interval.into()));
            flush_stdout.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

            // skip the first tick
            flush_disk.tick().await;
            flush_stdout.tick().await;
            write_binary(&mut cast_file, &timestamp.to_le_bytes()).ok();
            if verbose_log {
                buf_stdout.extend_from_slice(&timestamp.to_le_bytes());
            }

            let (mut rows, mut cols) = stty_size;

            loop {
                tokio::select! {
                    Some(evt) = cast_rx.recv() => {
                        match evt.kind {
                            EventKind::Input => {
                                let bytes = encode_evt(&evt);
                                write_binary(&mut cast_file, &bytes).ok();
                            }
                            EventKind::Resize => {
                                let bytes = encode_evt(&evt);
                                write_binary(&mut cast_file, &bytes).ok();
                                rows = u16::from_le_bytes([evt.payload[0], evt.payload[1]]);
                                cols = u16::from_le_bytes([evt.payload[2], evt.payload[3]]);
                                if verbose_log {
                                    buf_stdout.extend_from_slice(&bytes);
                                }
                            }
                            EventKind::Output => {
                                buf_disk.extend_from_slice(evt.payload.as_slice());
                            }
                        }
                    }

                    Some(ts)  = hb_rx.recv() => {
                        hb_file.write_all(&ts.to_le_bytes()).unwrap();
                        hb_file.flush().ok();
                    }

                    _ = flush_disk.tick() => {
                        if !buf_disk.is_empty() {
                            let idx = buf_trim(&buf_disk, cols, rows as u32 + 20);
                            let trimmed = &buf_disk[idx..];
                            let evt = RawEvt {
                                elapsed: start.elapsed().as_secs_f32(),
                                kind: EventKind::Output,
                                payload: trimmed.to_vec(),
                            };
                            let bytes = encode_evt(&evt);
                            write_binary(&mut cast_file, &bytes).ok();
                            if verbose_log {
                                buf_stdout.extend_from_slice(&bytes);
                            }
                            buf_disk.clear();
                        }
                    }
                    _ = flush_stdout.tick(), if verbose_log => {
                        if !buf_stdout.is_empty() {
                            match encode_all(&buf_stdout[..], 3) {
                                Ok(cmp) => {
                                    let b64 = base64::engine::general_purpose::STANDARD.encode(&cmp);
                                    let payload = serde_json::json!([timestamp, b64]);
                                    logger("cast", payload);
                                }
                                Err(e) => {
                                    logger("error", serde_json::json!(format!("Error encoding cast data: {}", e.to_string())));
                                }
                            }
                            buf_stdout.clear();
                        }
                    }

                    else => break,
                }
            }

            let _ = cast_file.flush();
            let _ = hb_file.flush();
        });

        Ok(Arc::new(Self { cast_tx, hb_tx }))
    }

    pub fn input(&self, elapsed: f32, bytes: Vec<u8>) {
        self.cast_tx
            .send(RawEvt {
                elapsed,
                kind: EventKind::Input,
                payload: bytes,
            })
            .ok();
    }
    pub fn output(&self, elapsed: f32, bytes: Vec<u8>) {
        self.cast_tx
            .send(RawEvt {
                elapsed,
                kind: EventKind::Output,
                payload: bytes,
            })
            .ok();
    }
    pub fn resize(&self, elapsed: f32, rows: u16, cols: u16) {
        let mut p = Vec::with_capacity(4);
        p.extend_from_slice(&rows.to_le_bytes());
        p.extend_from_slice(&cols.to_le_bytes());
        self.cast_tx
            .send(RawEvt {
                elapsed,
                kind: EventKind::Resize,
                payload: p,
            })
            .ok();
    }
    pub fn heartbeat(&self) {
        let ts_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs() as u32;
        self.hb_tx.send(ts_sec).ok();
    }
}
