use crate::models::RingBytes;
use anyhow::{Context, Result};
use portable_pty::*;
use std::{
    io::{Read, Write},
    sync::Arc,
};
use tokio::{
    sync::{Mutex, broadcast},
    task,
};

const BUF_SIZE: usize = 4096;

pub struct PtyManager {
    tx: broadcast::Sender<Vec<u8>>,
    history: Arc<Mutex<RingBytes>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    size: Arc<Mutex<PtySize>>,
}

impl PtyManager {
    pub async fn new(rows: u16, cols: u16, history_limit: usize) -> Result<Self> {
        let (tx, _) = broadcast::channel::<Vec<u8>>(4096);
        let history = Arc::new(Mutex::new(RingBytes::new(history_limit)));
        let size = Arc::new(Mutex::new(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }));

        let (writer, master, _child) = Self::spawn_shell(&size).await?;
        let writer = Arc::new(Mutex::new(writer));
        let master = Arc::new(Mutex::new(master));

        Self::launch_reader(
            tx.clone(),
            Arc::clone(&history),
            Arc::clone(&writer),
            Arc::clone(&master),
            Arc::clone(&size),
        );

        Ok(Self {
            tx,
            history,
            writer,
            master,
            size,
        })
    }

    pub async fn subscribe(&self) -> (broadcast::Receiver<Vec<u8>>, RingBytes) {
        (self.tx.subscribe(), self.history.lock().await.clone())
    }

    pub async fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().await;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub async fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let mut sz = self.size.lock().await;
        if sz.rows == rows && sz.cols == cols {
            return Ok(());
        }
        sz.rows = rows;
        sz.cols = cols;
        self.master.lock().await.resize(*sz)?;
        Ok(())
    }

    async fn spawn_shell(
        size: &Arc<Mutex<PtySize>>,
    ) -> Result<(Box<dyn Write + Send>, Box<dyn MasterPty + Send>, Box<dyn Child + Send>)> {
        let sz = *size.lock().await;
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(sz).context("open pty")?;

        let mut cmd = CommandBuilder::new("/bin/bash");
        cmd.env("LC_CTYPE", "C.UTF-8");
        cmd.env("TERM", "xterm-color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair.slave.spawn_command(cmd).context("spawn shell")?;
        let writer = pair.master.take_writer().context("take writer")?;
        Ok((writer, pair.master, child))
    }

    fn launch_reader(
        tx: broadcast::Sender<Vec<u8>>,
        history: Arc<Mutex<RingBytes>>,
        writer: Arc<Mutex<Box<dyn Write + Send>>>,
        master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
        size: Arc<Mutex<PtySize>>,
    ) {
        task::spawn_blocking(move || {
            loop {
                let mut reader = master.blocking_lock().try_clone_reader().expect("clone reader");

                let mut buf = [0u8; BUF_SIZE];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            history.blocking_lock().extend(&buf[..n]);
                            let _ = tx.send(buf[..n].to_vec());
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(_) => break,
                    }
                }

                const COMPLETED: &[u8] = b"[Process completed]\r\n\r\n";
                {
                    history.blocking_lock().extend(COMPLETED);
                    let _ = tx.send(COMPLETED.to_vec());
                }

                match tokio::runtime::Handle::current().block_on(Self::spawn_shell(&size)) {
                    Ok((new_writer, new_master, _new_child)) => {
                        *writer.blocking_lock() = new_writer;
                        *master.blocking_lock() = new_master;
                    }
                    Err(e) => {
                        let msg = format!("[Respawn failed: {e}]\r\n").into_bytes();
                        let _ = tx.send(msg.clone());
                        history.blocking_lock().extend(&msg);
                        break;
                    }
                }
            }
        });
    }
}
