// mirra (c) Nikolas Wipper 2022

use std::io::Result;
use std::path::Path;

use tokio::fs::OpenOptions;

use crate::{Client, Config, Environment};
use crate::packet::{ContinueSync, FileHeader, Handshake, Intent, Ok};

async fn receive_sync(client: &mut Client) -> Result<()> {
    loop {
        let cont: ContinueSync = client.expect().await?;
        client.send(Ok::new()).await?;
        if !cont.cont { break; }

        let header: FileHeader = client.expect().await?;
        client.send(Ok::new()).await?;

        println!("{}\n{}\n{}", header.path, header.hash, header.cert);

        let file = OpenOptions::new()
            .write(true)
            .read(false)
            .truncate(true)
            .create(true)
            .open(Path::new(&header.path)).await?;
        println!("opened");
        client.expect_file(file).await?;

        println!("saved");

        client.send(Ok::new()).await?;
    }

    Ok(())
}

pub async fn node(config: Config, _env: Environment) -> Result<()> {
    let mut client = Client::new("0.0.0.0".to_string(), 6007).await?;

    client.send(Handshake::new(config.name, client.local_addr().ip().to_string())).await?;
    client.expect::<Ok>().await?;

    client.send(Intent::FullSync).await?;
    client.expect::<Ok>().await?;

    receive_sync(&mut client).await
}
