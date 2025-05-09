use anyhow::Context;
use base64::Engine as _;
use clap::Parser;
use flate2::{Compression, write::GzEncoder};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex, RwLock},
};

pub struct Tailer {
    path: PathBuf,
    offset: u64,
    dev_ino: (u64, u64),
    file: File,
}

impl Tailer {
    // returns (file, (dev, ino))
    fn open_file(path: &PathBuf) -> anyhow::Result<(File, (u64, u64))> {
        let file = File::open(path).with_context(|| format!("open {:?}", path))?;
        let meta = file.metadata()?;
        Ok((file, (meta.dev(), meta.ino())))
    }

    fn need_reopen(&self) -> anyhow::Result<bool> {
        let meta = self.path.metadata()?;
        Ok((meta.dev(), meta.ino()) != self.dev_ino || meta.len() < self.offset)
    }

    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        let (file, dev_ino) = Tailer::open_file(&path)?;
        Ok(Self {
            path,
            offset: 0,
            dev_ino,
            file,
        })
    }

    pub fn poll(&mut self) -> anyhow::Result<Option<String>> {
        if self.need_reopen()? {
            let (file, dev_ino) = Self::open_file(&self.path)?;
            self.file = file;
            self.dev_ino = dev_ino;
            self.offset = 0;
        }
        self.file.seek(SeekFrom::Start(self.offset))?;

        let mut buf = String::new();
        self.file.read_to_string(&mut buf)?;
        self.offset = self.file.stream_position()?;

        if buf.is_empty() {
            return Ok(None);
        }
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        std::io::copy(&mut buf.as_bytes(), &mut enc)?;
        let compressed = enc.finish()?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(compressed);
        Ok(Some(b64))
    }
}

type TailerMap = RwLock<HashMap<PathBuf, Arc<Mutex<Tailer>>>>;
static HEARTBEATS: Lazy<Mutex<Vec<(u64, u32)>>> = Lazy::new(|| Mutex::new(Vec::new()));
static TAILERS: Lazy<TailerMap> = Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum ClientCmd {
    Info { msg: String },
    Warning { msg: String },
    Error { msg: String },
    CastPoll { cast: String },
    Hb { ts: u64, session: u32 },
    HeartbeatPoll,
}

fn logger<P>(kind: &str, payload: P)
where
    P: Serialize,
{
    let line = json!([kind, payload]).to_string();
    println!("{line}");
    std::io::stdout().flush().ok();
}

async fn handle_client(stream: UnixStream) -> anyhow::Result<()> {
    let (read_half, _write_half_unused) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let cmd: ClientCmd = match serde_json::from_str(&line) {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("json parse error: {e:#}");
                logger("error", &err_msg);
                continue;
            }
        };
        process_cmd(cmd).await?;
    }
    Ok(())
}

async fn process_cmd(cmd: ClientCmd) -> anyhow::Result<()> {
    match cmd {
        ClientCmd::Info { msg } => {
            logger("info", &msg);
        }
        ClientCmd::Warning { msg } => {
            logger("warning", &msg);
        }
        ClientCmd::Error { msg } => {
            logger("error", &msg);
        }
        ClientCmd::Hb { ts, session } => {
            HEARTBEATS.lock().await.push((ts, session));
        }
        ClientCmd::HeartbeatPoll => {
            let mut hb = HEARTBEATS.lock().await;
            if !hb.is_empty() {
                let payload = std::mem::take(&mut *hb);
                logger("heartbeat", payload);
            }
        }
        ClientCmd::CastPoll { cast } => {
            let filename = cast.clone();
            let path = PathBuf::from(cast);
            let tailer_arc = {
                use std::collections::hash_map::Entry;
                let mut map = TAILERS.write().await;
                match map.entry(path.clone()) {
                    Entry::Occupied(o) => o.get().clone(),
                    Entry::Vacant(v) => {
                        let tailer =
                            Tailer::new(path.clone()).with_context(|| format!("init tailer for {:?}", path))?;
                        let arc = Arc::new(Mutex::new(tailer));
                        v.insert(arc.clone());
                        arc
                    }
                }
            };
            let mut guard = tailer_arc.lock().await;
            if let Some(b64) = guard.poll()? {
                logger("cast", [&filename, &b64]);
            }
        }
    }
    Ok(())
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "/tmp/workspace-logger.sock")]
    socket: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let _ = std::fs::remove_file(&args.socket);
    let listener = UnixListener::bind(&args.socket)?;
    logger("info", format!("Unix Socket on {}", args.socket));

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream).await {
                let err_msg = format!("handle_client error: {e:#}");
                logger("error", &err_msg);
            }
        });
    }
}
