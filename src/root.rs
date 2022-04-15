// mirra (c) Nikolas Wipper 2022

use std::ffi::OsStr;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, SystemTime};

use tokio::fs;
use async_recursion::async_recursion;
use log::{info, warn};
use notify::{DebouncedEvent, RecursiveMode, Watcher};
use tokio::fs::File;

use crate::{Client, Server};
use crate::config::Config;
use crate::keys::LocalKeys;
use crate::packet::{BeginSync, Close, EndSync, FileHeader, Handshake, Ok, PacketKind, Heartbeat, NotFound, Remove, Rename};
use crate::util::{AsyncFileLock, hash_file, stringify};

async fn sync_file(socket: &mut Client, outof: PathBuf, path: &Path, keys: Arc<LocalKeys>) -> Result<()> {
    let relative_path = stringify(path.strip_prefix(outof.clone()).unwrap())?;
    info!("Syncing {}", relative_path);

    let mut file = File::open(path).await?;
    file.lock().await?;

    let hash = hash_file(&mut file).await?;

    socket.send(FileHeader::new(relative_path, hash.clone(), keys.sign(hash))).await?;

    let next = socket.read_packet_kind().await?;
    match next {
        PacketKind::Ok => {}
        PacketKind::Skip | PacketKind::Close => {
            return Ok(());
        }
        _ => {
            return Err(Error::new(ErrorKind::InvalidData, "unexpected package"));
        }
    }

    socket.send_file(&mut file).await?;
    file.unlock().await?;

    socket.expect::<Ok>().await?;

    Ok(())
}

#[async_recursion]
async fn sync_dir(socket: &mut Client, root_dir: PathBuf, dir: PathBuf, keys: Arc<LocalKeys>) -> Result<()> {
    if dir.as_path().file_name().unwrap() == OsStr::new(".mirra") {
        info!("Skipping {}", dir.to_str().unwrap_or("mirra directory"));
        return Ok(());
    }

    info!("Syncing directory {}", dir.to_str().unwrap_or("<couldnt read path>"));
    let mut list = tokio::fs::read_dir(dir).await?;
    loop {
        let entry = list.next_entry().await?;
        if entry.is_none() { break; }
        if let Some(entry) = entry {
            if entry.path().is_file() {
                sync_file(socket, root_dir.clone(), entry.path().as_path(), keys.clone()).await?;
            } else if entry.path().is_dir() {
                sync_dir(socket, root_dir.clone(), entry.path(), keys.clone()).await?;
            }
        }
    }

    Ok(())
}

async fn process_full_sync(socket: &mut Client, dir: PathBuf, keys: Arc<LocalKeys>) -> Result<()> {
    info!("Performing a sync");
    socket.send(BeginSync::new()).await?;
    socket.expect::<Ok>().await?;

    sync_dir(socket, dir.clone(), dir, keys).await?;

    socket.send(EndSync::new()).await?;

    socket.expect::<Ok>().await?;
    // connection is closed after this
    Ok(())
}

async fn process_socket(mut socket: Client, config: Arc<Config>, keys: Arc<LocalKeys>) -> Result<()> {
    let remote = socket.peer_addr();
    info!("Connected with {}", remote.ip());

    let mut module: String;
    let dir: PathBuf;

    loop {
        let first = socket.read_packet_kind().await?;
        match first {
            PacketKind::Handshake => {
                let handshake: Handshake = socket.expect_unchecked().await?;

                socket.send(Ok::new()).await?;

                info!("Performed handshake");

                module = handshake.module;
                if let Some(share) = config.shares.get(&module) {
                    dir = fs::canonicalize(PathBuf::from(share.path.clone())).await?;
                    break;
                } else if let Some(sync) = config.syncs.get(&module) {
                    dir = fs::canonicalize(PathBuf::from(sync.path.clone())).await?;
                    break;
                } else {
                    socket.send(NotFound::new()).await?;
                }
            }
            PacketKind::Close => {
                socket.send(Close::new()).await?;
                return Ok(());
            }
            _ => {
                return Err(Error::from(ErrorKind::InvalidData));
            }
        }
    }

    process_full_sync(&mut socket, dir.clone(), keys.clone()).await?;

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::watcher(tx, Duration::from_secs(1)).unwrap();

    watcher.watch(dir.clone(), RecursiveMode::Recursive).unwrap();

    let mut last_heartbeat = SystemTime::now();

    loop {
        let event = rx.try_recv();
        if event.is_err() {
            if event.as_ref().err().unwrap() == &TryRecvError::Empty {
                let now = SystemTime::now();
                if now.duration_since(last_heartbeat).unwrap() > Duration::from_secs(20) {
                    last_heartbeat = now;
                    socket.send(Heartbeat::new()).await?;

                    let next = socket.read_packet_kind().await?;
                    match next {
                        PacketKind::Ok => {}
                        PacketKind::Close => {
                            socket.send(Close::new()).await?;
                            return Ok(());
                        }
                        _ => {
                            return Err(Error::from(ErrorKind::InvalidData));
                        }
                    }
                }
            } else if let Err(e) = event {
                println!("watch error: {}", e.to_string());
            }
            continue;
        }

        match event.unwrap() {
            DebouncedEvent::Create(path) | DebouncedEvent::Write(path) => {
                info!("Dispatching file update event: {}", stringify(&path)?);
                sync_file(&mut socket, dir.clone(), path.as_path(), keys.clone()).await?;
            }
            DebouncedEvent::Remove(path) => {
                info!("Dispatching remove event: {}", stringify(&path)?);
                socket.send(Remove::new(stringify(path.strip_prefix(dir.clone()).unwrap())?)).await?;
                socket.expect::<Ok>().await?;
            }
            DebouncedEvent::Rename(old, new) => {
                info!("Dispatching rename event: {} -> {}", stringify(&old)?, stringify(&new)?);
                socket.send(Rename::new(stringify(old.strip_prefix(dir.clone()).unwrap())?,
                    stringify(new.strip_prefix(dir.clone()).unwrap())?)).await?;
                socket.expect::<Ok>().await?;
            }
            DebouncedEvent::Rescan => process_full_sync(&mut socket, dir.clone(), keys.clone()).await?,
            DebouncedEvent::Error(_, _) => {}
            _ => {}
        }
    }
}

pub async fn root(config: Arc<Config>, keys: Arc<LocalKeys>) -> Result<()> {
    let mut server = Server::new(config.port).await?;

    loop {
        let socket = server.accept().await?;

        let local_keys = keys.clone();
        let local_config = config.clone();
        tokio::spawn(async move {
            let r = process_socket(socket, local_config, local_keys).await;
            if r.is_err() {
                warn!("{}", r.err().unwrap().to_string());
            }
        });
    }
}
