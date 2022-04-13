// mirra (c) Nikolas Wipper 2022

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use async_recursion::async_recursion;
use log::{debug, info, warn};
use tokio::fs::File;

use crate::{Client, Config, Server};
use crate::environment::Environment;
use crate::packet::{Ok, ContinueSync, FileHeader, Handshake, Intent};

async fn sync_file(socket: &mut Client, path: &Path, _config: &Config, env: &Environment) -> Result<()> {
    debug!("Syncing {}", path.to_str().unwrap_or("<couldnt read path>"));

    socket.send(ContinueSync::new(true)).await?;

    socket.expect::<Ok>().await?;

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    let hash = hasher.finish().to_string();

    socket.send(FileHeader::new(path.to_str().unwrap().to_string(), hash.clone(), env.sign(hash))).await?;

    socket.expect::<Ok>().await?;

    socket.send(File::open(path).await?).await?;

    socket.expect::<Ok>().await?;

    Ok(())
}

#[async_recursion]
async fn sync_dir(socket: &mut Client, dir: PathBuf, config: &Config, env: &Environment) -> Result<()> {
    if dir == env::current_dir()?.join(".mirra") {
        debug!("Skipping {}", dir.to_str().unwrap_or("mirra directory"));
        return Ok(());
    }

    debug!("Syncing directory {}", dir.to_str().unwrap_or("<couldnt read path>"));
    let mut list = tokio::fs::read_dir(dir).await?;
    loop {
        let entry = list.next_entry().await?;
        if entry.is_none() { break; }
        if let Some(entry) = entry {
            if entry.path().is_file() {
                sync_file(socket, entry.path().strip_prefix(env::current_dir()?).unwrap(), config, env).await?;
            } else if entry.path().is_dir() {
                sync_dir(socket, entry.path(), config, env).await?;
            }
        }
    }

    Ok(())
}

async fn process_full_sync(mut socket: Client, config: &Config, env: &Environment) -> Result<()> {
    debug!("Performing a full sync");
    socket.send(Ok::new()).await?;

    let data_dir = env::current_dir()?;

    sync_dir(&mut socket, data_dir, config, env).await?;

    socket.send(ContinueSync::new(false)).await?;

    socket.expect::<Ok>().await?;
    // connection is closed after this
    Ok(())
}

async fn process_partial_sync(mut socket: Client, _config: &Config, _env: &Environment) -> Result<()> {
    socket.send(Ok::new()).await?;
    Ok(())
}


async fn process_certificate_sync(mut socket: Client, _config: &Config, _env: &Environment) -> Result<()> {
    socket.send(Ok::new()).await?;
    Ok(())
}

async fn process_socket(mut socket: Client, config: &Config, env: &Environment) -> Result<()> {
    let remote = socket.peer_addr();
    debug!("Connected with {}", remote.ip());

    let handshake: Handshake = socket.expect().await?;

    if remote.ip().to_string() != handshake.ip {
        info!("Couldn't verify node IP address");
        return Err(Error::from(ErrorKind::InvalidData));
    }

    socket.send(Ok::new()).await?;

    debug!("Performed handshake");

    let intent: Intent = socket.expect().await?;

    match intent {
        Intent::FullSync => process_full_sync(socket, config, env).await,
        Intent::PartialSync => process_partial_sync(socket, config, env).await,
        Intent::CertificateSync => process_certificate_sync(socket, config, env).await,
    }
}

pub async fn root(config: Config, env: Environment) -> Result<()> {
    let mut server = Server::new(config.port).await?;

    loop {
        let socket = server.accept().await?;
        let r = process_socket(socket, &config, &env).await;
        if r.is_err() {
            warn!("{}", r.err().unwrap().to_string());
        }
    }
}
