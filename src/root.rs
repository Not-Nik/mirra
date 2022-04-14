// mirra (c) Nikolas Wipper 2022

use std::collections::hash_map::DefaultHasher;
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_recursion::async_recursion;
use log::{info, warn};
use tokio::fs::File;

use crate::{Client, Server};
use crate::config::Config;
use crate::keys::LocalKeys;
use crate::packet::{Close, ContinueSync, Sync, FileHeader, Handshake, IndexRoot, Ok, PacketKind};

async fn sync_file(socket: &mut Client, outof: PathBuf, path: &Path, keys: Arc<LocalKeys>) -> Result<()> {
    info!("Syncing {}", path.to_str().unwrap_or("<couldnt read path>"));

    socket.send(ContinueSync::new(true)).await?;

    socket.expect::<Ok>().await?;

    let file_path = outof.join(path);

    let mut hasher = DefaultHasher::new();
    file_path.hash(&mut hasher);
    let hash = hasher.finish().to_string();

    socket.send(FileHeader::new(path.to_str().unwrap().to_string(), hash.clone(), keys.sign(hash))).await?;

    let next = socket.read_packet_kind().await?;
    match next {
        PacketKind::Ok => {}
        PacketKind::Skip => {
            return Ok(());
        }
        _ => {
            return Err(Error::new(ErrorKind::InvalidData, "unexpected package"));
        }
    }

    socket.send_file(File::open(file_path).await?).await?;

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
                sync_file(socket, root_dir.clone(), entry.path().strip_prefix(root_dir.clone()).unwrap(), keys.clone()).await?;
            } else if entry.path().is_dir() {
                sync_dir(socket, root_dir.clone(), entry.path(), keys.clone()).await?;
            }
        }
    }

    Ok(())
}

async fn process_sync(socket: &mut Client, dir: PathBuf, keys: Arc<LocalKeys>) -> Result<()> {
    info!("Performing a sync");
    sync_dir(socket, dir.clone(), dir, keys).await?;

    socket.send(ContinueSync::new(false)).await?;

    socket.expect::<Ok>().await?;
    // connection is closed after this
    Ok(())
}

async fn process_socket(mut socket: Client, config: Arc<Config>, keys: Arc<LocalKeys>) -> Result<()> {
    let remote = socket.peer_addr();
    info!("Connected with {}", remote.ip());

    socket.expect::<Handshake>().await?;

    socket.send(Ok::new()).await?;

    info!("Performed handshake");

    loop {
        let next = socket.read_packet_kind().await?;

        match next {
            PacketKind::Close => {
                info!("Closing connection");
                socket.send(Close::new()).await?;
                return Ok(());
            }
            PacketKind::Index => {
                let mut modules = Vec::with_capacity(config.shares.len());
                for share in &config.shares {
                    modules.push(share.0.clone());
                }
                socket.send(IndexRoot::new(modules)).await?;
            }
            PacketKind::Sync => {
                let sync: Sync = socket.expect_unchecked().await?;

                let dir: PathBuf;

                if let Some(share) = config.shares.get(&sync.module) {
                    dir = PathBuf::from(share.path.clone());
                } else if let Some(sync) = config.syncs.get(&sync.module) {
                    dir = PathBuf::from(sync.path.clone());
                } else {
                    return Err(Error::from(ErrorKind::InvalidData));
                }

                process_sync(&mut socket, dir, keys.clone()).await?
            }
            _ => {
                return Err(Error::from(ErrorKind::InvalidData));
            }
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
