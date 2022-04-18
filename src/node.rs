// mirra (c) Nikolas Wipper 2022

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

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

/// Receive a file from a remote mirra
async fn receive_file(client: &mut Client, header: FileHeader, into: PathBuf) -> Result<()> {
    // Create absolute file path from received header path and local destination directory
    let file_path = into.join(&header.path);
    // Check if the file is already on dist
    if file_path.exists() {
        // Open and lock file for hashing
        let mut file = File::open(file_path.clone()).await?;
        file.lock().await?;
        let hash = hash_file(&mut file).await?;
        file.unlock().await?;

        // File is already on disk
        if hash == header.hash {
            info!("Skipping {}, already on disk", header.path);
            client.send(Skip::new()).await?;
            return Ok(());
        }
    }

    // config
    client.send(Ok::new()).await?;

    // If the file is in a directory that previously didnt exist, create that
    if file_path.parent().is_some() && !file_path.parent().unwrap().exists() {
        fs::create_dir_all(file_path.parent().unwrap()).await?;
    }

    // Create/overwrite file
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

/// Sync the entire remote module
async fn receive_sync(client: &mut Client, into: PathBuf) -> Result<()> {
    loop {
        let next = client.read_packet_kind().await?;
        // Remote mirra has gone through all files
        if next == PacketKind::EndSync {
            // Acknowledge and return
            client.send(Ok::new()).await?;
            break;
        // Only [PacketKind::EndSync] and [PacketKind::FileHeader] are valid
        } else if next != PacketKind::FileHeader {
            return Err(Error::from(ErrorKind::InvalidData));
        }

        // Receive another file from the remote mirra
        let header: FileHeader = client.expect_unchecked().await?;
        receive_file(client, header, into.clone()).await?;
    }

    Ok(())
}

/// The main node lifecycle
pub async fn process_node(module: String, sync: RootSync) -> Result<()> {
    // Connect to remote mirra
    let mut client = Client::new(sync.address.clone() + ":" + &sync.port.to_string()).await?;
    info!("Connected to {}", sync.address);

    // Send handshake
    client.send(Handshake::new(module.clone())).await?;

    let status = client.read_packet_kind().await?;
    // Close if remote mirra doesn't have the requested module
    if status == PacketKind::NotFound {
        info!("{} not found on remote mirra", module);
        client.close().await?;
        return Err(Error::from(ErrorKind::InvalidInput));
    } else if status != PacketKind::Ok {
        return Err(Error::from(ErrorKind::InvalidData));
    }

    info!("Performed handshake");

    // Create target directory if it doesn't exist
    let dir = PathBuf::from(sync.path);
    if !dir.exists() {
        fs::create_dir_all(dir.clone()).await?;
    }

    loop {
        let next = client.read_packet_kind().await?;

        match next {
            // Just a heartbeat, acknowledge and continue
            PacketKind::Heartbeat => {
                client.send(Ok::new()).await?;
                debug!("Heartbeat");
            }
            // Sync the entire module
            PacketKind::BeginSync => {
                client.send(Ok::new()).await?;
                info!("Performing a full sync");
                receive_sync(&mut client, dir.clone()).await?;
            }
            // Sync a single file
            PacketKind::FileHeader => {
                info!("Single file sync");
                let header = client.expect_unchecked().await?;
                receive_file(&mut client, header, dir.clone()).await?;
            }
            // Remove a file
            PacketKind::Remove => {
                let remove: Remove = client.expect_unchecked().await?;
                client.send(Ok::new()).await?;

                info!("Removing {}", remove.path.clone());

                let path = dir.join(remove.path);
                // Ignore files that are already deleted, and directories
                if path.exists() && path.is_file() && fs::remove_file(path.clone()).await.is_err() {
                    warn!("Failed to delete {} due to lack of permissions", stringify(&path)?);
                }
            }
            // Rename a file
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

/// Create a node process for every module that needs to synced from a remote mirra
pub async fn node(config: Arc<Config>, _env: Arc<LocalKeys>) -> Result<()> {
    let mut futs = Vec::with_capacity(config.syncs.len());

    for sync in &config.syncs {
        futs.push(tokio::spawn(process_node(sync.0.clone(), sync.1.clone())));
    }
    for fut in futs {
        fut.await??;
    }

    Ok(())
}
