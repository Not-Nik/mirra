// mirra (c) Nikolas Wipper 2022

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

/// Send a file to a remote mirra node
async fn sync_file(socket: &mut Client, outof: PathBuf, path: &Path, keys: Arc<LocalKeys>) -> Result<()> {
    // Make path relative, so the node knows where to put it
    let relative_path = stringify(path.strip_prefix(outof.clone()).unwrap())?;
    info!("Syncing {}", relative_path);

    // Open and lock file
    let mut file = File::open(path).await?;
    file.lock().await?;

    // Hash file
    let hash = hash_file(&mut file).await?;

    // Send file metadata
    socket.send(FileHeader::new(relative_path, hash.clone(), keys.sign(hash))).await?;

    let next = socket.read_packet_kind().await?;
    match next {
        PacketKind::Ok => {}
        // Skip file if it already exists on the node
        PacketKind::Skip | PacketKind::Close => {
            return Ok(());
        }
        _ => {
            return Err(Error::new(ErrorKind::InvalidData, "unexpected package"));
        }
    }

    // Send file
    socket.send_file(&mut file).await?;
    file.unlock().await?;

    socket.expect::<Ok>().await?;

    Ok(())
}

/// Sync a directory to a remote mirra node
#[async_recursion]
async fn sync_dir(socket: &mut Client, root_dir: PathBuf, dir: PathBuf, keys: Arc<LocalKeys>) -> Result<()> {
    info!("Syncing directory {}", dir.to_str().unwrap_or("<couldnt read path>"));
    // Go through each entry (tokio's ReadDir doesn't support iter)
    let mut list = tokio::fs::read_dir(dir).await?;
    loop {
        // Get next directory entry
        let entry = list.next_entry().await?;
        if entry.is_none() { break; }
        if let Some(entry) = entry {
            if entry.path().is_file() {
                // Send file directly
                sync_file(socket, root_dir.clone(), entry.path().as_path(), keys.clone()).await?;
            } else if entry.path().is_dir() {
                // Sync directories recursively
                sync_dir(socket, root_dir.clone(), entry.path(), keys.clone()).await?;
            }
        }
    }

    Ok(())
}

/// Sync an entire module to a remote mirra node
async fn process_full_sync(socket: &mut Client, dir: PathBuf, keys: Arc<LocalKeys>) -> Result<()> {
    info!("Performing a sync");
    // Tell the node
    socket.send(BeginSync::new()).await?;
    socket.expect::<Ok>().await?;

    // Sync the root dir
    sync_dir(socket, dir.clone(), dir, keys).await?;

    // Tell the node it's over :)
    socket.send(EndSync::new()).await?;

    socket.expect::<Ok>().await?;
    Ok(())
}

/// Main lifecycle of a connection to a node
async fn process_socket(mut socket: Client, config: Arc<Config>, keys: Arc<LocalKeys>) -> Result<()> {
    let remote = socket.peer_addr();
    info!("Connected with {}", remote.ip());

    let mut module: String;
    let dir: PathBuf;

    // Handshake with the node
    loop {
        let first = socket.read_packet_kind().await?;
        match first {
            PacketKind::Handshake => {
                let handshake: Handshake = socket.expect_unchecked().await?;

                socket.send(Ok::new()).await?;

                info!("Performed handshake");

                module = handshake.module;
                if let Some(share) = config.shares.get(&module) {
                    // Save an absolute path
                    dir = fs::canonicalize(PathBuf::from(share.path.clone())).await?;
                    break;
                } else if let Some(sync) = config.syncs.get(&module) {
                    // Save an absolute path
                    dir = fs::canonicalize(PathBuf::from(sync.path.clone())).await?;
                    break;
                } else {
                    // The requested module wasn't found
                    // After this the loop continues, giving the node another chance
                    socket.send(NotFound::new()).await?;
                }
            }
            PacketKind::Close => {
                // Node gave up, likely after a `NotFound` package
                socket.send(Close::new()).await?;
                return Ok(());
            }
            _ => {
                return Err(Error::from(ErrorKind::InvalidData));
            }
        }
    }

    // Sync the entire module at first
    process_full_sync(&mut socket, dir.clone(), keys.clone()).await?;

    // Watch the module for any changes to files
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::watcher(tx, Duration::from_secs(1)).unwrap();
    // note: this creates a new thread
    watcher.watch(dir.clone(), RecursiveMode::Recursive).unwrap();

    let mut last_heartbeat = SystemTime::now();

    // Main loop
    loop {
        // This gives us an Err if there are no events
        // giving us time to do heartbeating
        let event = rx.try_recv();
        if event.is_err() {
            if event.as_ref().err().unwrap() == &TryRecvError::Empty {
                let now = SystemTime::now();
                // Send a heartbeat every 20 seconds
                if now.duration_since(last_heartbeat).unwrap() > Duration::from_secs(20) {
                    // Reset timer
                    last_heartbeat = now;
                    socket.send(Heartbeat::new()).await?;

                    let next = socket.read_packet_kind().await?;
                    match next {
                        // The node should acknowledge, but you never know
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

        // Handle any changes
        match event.unwrap() {
            // Create and write are basically the same
            DebouncedEvent::Create(path) | DebouncedEvent::Write(path) => {
                info!("Dispatching file update event: {}", stringify(&path)?);
                sync_file(&mut socket, dir.clone(), path.as_path(), keys.clone()).await?;
            }
            // Remove is rather trivial
            DebouncedEvent::Remove(path) => {
                info!("Dispatching remove event: {}", stringify(&path)?);
                socket.send(Remove::new(stringify(path.strip_prefix(dir.clone()).unwrap())?)).await?;
                socket.expect::<Ok>().await?;
            }
            // Rename is rather trivial
            DebouncedEvent::Rename(old, new) => {
                info!("Dispatching rename event: {} -> {}", stringify(&old)?, stringify(&new)?);
                socket.send(Rename::new(stringify(old.strip_prefix(dir.clone()).unwrap())?,
                    stringify(new.strip_prefix(dir.clone()).unwrap())?)).await?;
                socket.expect::<Ok>().await?;
            }
            // Just resynchronise the entire thing to be share
            DebouncedEvent::Rescan => process_full_sync(&mut socket, dir.clone(), keys.clone()).await?,
            _ => {}
        }
    }
}

/// The main root lifecycle
pub async fn root(config: Arc<Config>, keys: Arc<LocalKeys>) -> Result<()> {
    let mut server = Server::new(config.port).await?;

    loop {
        // Accept a new connection
        let socket = server.accept().await?;

        // Get a new reference to config and keys
        let local_keys = keys.clone();
        let local_config = config.clone();
        // Create a new task for the [process_socket] call
        tokio::spawn(async move {
            let r = process_socket(socket, local_config, local_keys).await;
            if r.is_err() {
                warn!("{}", r.err().unwrap().to_string());
            }
        });
    }
}
