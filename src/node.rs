// mirra (c) Nikolas Wipper 2022

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Result;
use std::path::Path;
use log::info;

use tokio::fs::OpenOptions;

use crate::{Client, Config, LocalKeys};
use crate::packet::{ContinueSync, FileHeader, Sync, Ok, Close, Skip, Handshake};

async fn receive_sync(client: &mut Client) -> Result<()> {
    loop {
        let cont: ContinueSync = client.expect().await?;
        client.send(Ok::new()).await?;
        if !cont.cont { break; }

        let header: FileHeader = client.expect().await?;

        let file_path = Path::new(&header.path);
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

pub async fn node(config: Config, _env: LocalKeys) -> Result<()> {
    let root_ip = config.node_config.as_ref().unwrap().root_addr.clone();
    let root_port = config.node_config.as_ref().unwrap().root_port;

    let mut client = Client::new(root_ip.clone(), root_port).await?;
    info!("Connected to {}", root_ip);

    client.send(Handshake::new(config.name)).await?;
    client.expect::<Ok>().await?;

    info!("Performed handshake");

    client.send(Sync::new()).await?;
    client.expect::<Ok>().await?;

    info!("Syncing files");

    receive_sync(&mut client).await
}
