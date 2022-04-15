// mirra (c) Nikolas Wipper 2022

use std::env;
use std::io::Result;
use std::sync::Arc;
use log::debug;
use tokio::join;

use crate::config::get_config;
use crate::keys::{LocalKeys, get_environment};
use crate::socket::{Client, Server};

mod keys;
mod socket;
mod util;
mod root;
mod node;
mod packet;
mod config;

#[tokio::main]
async fn main() -> Result<()> {
    // hack to enable logging by default
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    let config = Arc::from(get_config().await?);
    let env = Arc::from(get_environment()?);

    debug!("{:?}", config);

    let root_fut = tokio::spawn(root::root(config.clone(), env.clone()));
    let node_fut = tokio::spawn(node::node(config.clone(), env.clone()));

    let (root_res, node_res) = join!(root_fut, node_fut);
    root_res??;
    node_res??;

    return Ok(());
}
