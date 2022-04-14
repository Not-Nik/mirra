// mirra (c) Nikolas Wipper 2022

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use async_recursion::async_recursion;
use log::{info, warn};
use tokio::fs::File;

use crate::{Client, Config, Server};
use crate::environment::LocalKeys;
use crate::packet::{Close, ContinueSync, FileHeader, Handshake, Ok, PacketKind};

async fn sync_file(socket: &mut Client, path: &Path, keys: &LocalKeys) -> Result<()> {
    info!("Syncing {}", path.to_str().unwrap_or("<couldnt read path>"));

    socket.send(ContinueSync::new(true)).await?;

    socket.expect::<Ok>().await?;

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
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

    socket.send(File::open(path).await?).await?;

    socket.expect::<Ok>().await?;

    Ok(())
}

#[async_recursion]
async fn sync_dir(socket: &mut Client, dir: PathBuf, keys: &LocalKeys) -> Result<()> {
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
                sync_file(socket, entry.path().strip_prefix(env::current_dir()?).unwrap(), keys).await?;
            } else if entry.path().is_dir() {
                sync_dir(socket, entry.path(), keys).await?;
            }
        }
    }

    Ok(())
}

async fn process_full_sync(socket: &mut Client, keys: &LocalKeys) -> Result<()> {
    info!("Performing a sync");
    socket.send(Ok::new()).await?;

    let data_dir = env::current_dir()?;

    sync_dir(socket, data_dir, keys).await?;

    socket.send(ContinueSync::new(false)).await?;

    socket.expect::<Ok>().await?;
    // connection is closed after this
    Ok(())
}

async fn process_socket(mut socket: Client, keys: &LocalKeys) -> Result<()> {
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
                return Ok(())
            }
            PacketKind::Sync => {
                process_full_sync(&mut socket, keys).await?
            }
            _ => {
                return Err(Error::from(ErrorKind::InvalidData));
            }
        }
    }
}

pub async fn root(config: Config, keys: LocalKeys) -> Result<()> {
    let mut server = Server::new(config.port).await?;

    loop {
        let socket = server.accept().await?;
        let r = process_socket(socket, &keys).await;
        if r.is_err() {
            warn!("{}", r.err().unwrap().to_string());
        }
    }
}
