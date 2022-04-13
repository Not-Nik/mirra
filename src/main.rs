// mirra (c) Nikolas Wipper 2022

use std::io::Result;

use crate::environment::{Config, Environment, get_config, get_environment};
use crate::socket::{Client, Server};

mod environment;
mod socket;
mod util;
mod root;
mod node;
mod packet;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let config = get_config()?;
    let env = get_environment()?;

    if config.is_root {
        root::root(config, env).await?;
    } else {
        node::node(config, env).await?;
    }

    return Ok(());
}
