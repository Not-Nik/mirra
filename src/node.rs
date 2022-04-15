// mirra (c) Nikolas Wipper 2022

use std::io::{Error, ErrorKind, Result};
use std::io::ErrorKind::InvalidData;
use std::path::PathBuf;
use std::sync::Arc;
use log::{debug, info, warn};

use tokio::fs;
use tokio::fs::{File, OpenOptions};

use crate::{Client, LocalKeys};
use crate::config::{Config, RootSync};
use crate::packet::{FileHeader, Ok, Skip, Handshake, PacketKind, Remove, Rename};
use crate::util::{AsyncFileLock, hash_file, stringify};

async fn receive_file(client: &mut Client, header: FileHeader, into: PathBuf) -> Result<()> {
    let file_path = into.join(&header.path);
    if file_path.exists() {
        let mut file = File::open(file_path.clone()).await?;
        file.lock().await?;
        let hash = hash_file(&mut file).await?;
        file.unlock().await?;

        if hash == header.hash {
            info!("Skipping {}, already on disk", header.path);
            client.send(Skip::new()).await?;
            return Ok(());
        }
    }

    client.send(Ok::new()).await?;

    if file_path.parent().is_some() && !file_path.parent().unwrap().exists() {
        fs::create_dir_all(file_path.parent().unwrap()).await?;
    }

    let file = OpenOptions::new()
        .write(true)
        .read(false)
        .truncate(true)
        .create(true)
        .open(file_path).await?;

    info!("Receiving {}", header.path);
    client.expect_file(file).await?;

    client.send(Ok::new()).await?;
    Ok(())
}

async fn receive_sync(client: &mut Client, into: PathBuf) -> Result<()> {
    loop {
        let next = client.read_packet_kind().await?;
        if next == PacketKind::EndSync {
            client.send(Ok::new()).await?;
            break;
        } else if next != PacketKind::FileHeader {
            return Err(Error::from(ErrorKind::InvalidData));
        }

        let header: FileHeader = client.expect_unchecked().await?;

        receive_file(client, header, into.clone()).await?;
    }

    Ok(())
}

pub async fn process_sync(module: String, sync: RootSync) -> Result<()> {
    let mut client = Client::new(sync.ip.clone(), sync.port).await?;
    info!("Connected to {}", sync.ip);

    client.send(Handshake::new(module.clone())).await?;

    let status = client.read_packet_kind().await?;
    if status == PacketKind::NotFound {
        info!("{} not found on remote mirra", module);
        client.close().await?;
        return Err(Error::from(ErrorKind::InvalidInput));
    } else if status != PacketKind::Ok {
        return Err(Error::from(ErrorKind::InvalidData));
    }

    info!("Performed handshake");

    let dir = PathBuf::from(sync.path);
    if !dir.exists() {
        fs::create_dir_all(dir.clone()).await?;
    }

    loop {
        let next = client.read_packet_kind().await?;

        match next {
            PacketKind::Heartbeat => {
                client.send(Ok::new()).await?;
                debug!("Heartbeat");
            }
            PacketKind::BeginSync => {
                client.send(Ok::new()).await?;
                info!("Performing a full sync");
                receive_sync(&mut client, dir.clone()).await?;
            }
            PacketKind::FileHeader => {
                info!("Single file sync");
                let header = client.expect_unchecked().await?;
                receive_file(&mut client, header, dir.clone()).await?;
            }
            PacketKind::Remove => {
                let remove: Remove = client.expect_unchecked().await?;
                client.send(Ok::new()).await?;

                info!("Removing {}", remove.path.clone());

                let path = dir.join(remove.path);
                if path.exists() && path.is_file() && fs::remove_file(path.clone()).await.is_err() {
                    warn!("Failed to delete {} due to lack of permissions", stringify(&path)?);
                }
            }
            PacketKind::Rename => {
                let rename: Rename = client.expect_unchecked().await?;
                client.send(Ok::new()).await?;

                info!("Renaming {} -> {}", rename.old.clone(), rename.new.clone());

                let res = fs::rename(dir.join(rename.old.clone()), dir.join(rename.new.clone())).await;
                if res.is_err() {
                    warn!("Failed to rename {} -> {}: {}", rename.old, rename.new, res.err().unwrap().to_string());
                }
            }
            _ => {
                // politely deny that
                client.close().await?;
                return Err(Error::from(InvalidData));
            }
        }
    }
}

pub async fn node(config: Arc<Config>, _env: Arc<LocalKeys>) -> Result<()> {
    let mut futs = Vec::with_capacity(config.syncs.len());

    for sync in &config.syncs {
        futs.push(tokio::spawn(process_sync(sync.0.clone(), sync.1.clone())));
    }
    for fut in futs {
        fut.await??;
    }

    Ok(())
}
