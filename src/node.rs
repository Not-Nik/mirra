// mirra (c) Nikolas Wipper 2022

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Error, ErrorKind, Result};
use std::path::PathBuf;
use std::sync::Arc;
use log::{error, info};

use tokio::fs;
use tokio::fs::OpenOptions;

use crate::{Client, LocalKeys};
use crate::config::{Config, RootSync};
use crate::packet::{ContinueSync, FileHeader, Sync, Ok, Close, Skip, Handshake, IndexNode, IndexRoot};

async fn receive_sync(client: &mut Client, into: PathBuf) -> Result<()> {
    loop {
        let cont: ContinueSync = client.expect().await?;
        client.send(Ok::new()).await?;
        if !cont.cont { break; }

        let header: FileHeader = client.expect().await?;

        let file_path = into.join(&header.path);
        if file_path.exists() {
            let mut hasher = DefaultHasher::new();
            file_path.hash(&mut hasher);
            let hash = hasher.finish().to_string();

            if hash == header.hash {
                info!("Skipping {}, already on disk", header.path);
                client.send(Skip::new()).await?;
                continue;
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
    }

    client.send(Close::new()).await?;
    client.expect::<Close>().await?;

    Ok(())
}

pub async fn process_sync(name: String, module: String, sync: RootSync) -> Result<()> {
    let mut client = Client::new(sync.ip.clone(), sync.port).await?;
    info!("Connected to {}", sync.ip);

    client.send(Handshake::new(name.clone())).await?;
    client.expect::<Ok>().await?;

    info!("Performed handshake");

    client.send(IndexNode::new()).await?;
    let index: IndexRoot = client.expect().await?;

    if !index.modules.contains(&module) {
        error!("Remote server doesn't have '{}'", module);
        client.send(Close::new()).await?;
        client.expect::<Close>().await?;
        return Err(Error::from(ErrorKind::InvalidInput));
    }

    client.send(Sync::new(module)).await?;

    info!("Syncing files");

    let dir = PathBuf::from(sync.path);
    if !dir.exists() {
        fs::create_dir_all(dir.clone()).await?;
    }
    receive_sync(&mut client, dir).await
}

pub async fn node(config: Arc<Config>, _env: Arc<LocalKeys>) -> Result<()> {
    let mut futs = Vec::with_capacity(config.syncs.len());

    for sync in &config.syncs {
        futs.push(tokio::spawn(process_sync(config.name.clone(), sync.0.clone(), sync.1.clone())));
    }
    for fut in futs {
        fut.await??;
    }

    Ok(())
}
