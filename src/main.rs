// mirra (c) Nikolas Wipper 2022

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

extern crate core;

use std::env;
use std::io::Result;
use std::sync::Arc;
use tokio::join;

use crate::config::get_config;
use crate::keys::{LocalKeys, get_keys};
use crate::socket::{Client, Server};

mod keys;
mod socket;
mod util;
mod root;
mod node;
mod packet;
mod config;
mod web;

#[tokio::main]
async fn main() -> Result<()> {
    // hack to enable logging by default
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    // Load config and keys from disk
    // Atomically refcounted, so we can use them with [tokio::spawn], which might
    // move tasks between threads with feature "rt-multi-thread" enabled
    let config = Arc::from(get_config().await?);
    let env = Arc::from(get_keys()?);

    // Start root and node servers
    // See [root::root]'s and [node::node]'s descriptions for more info
    let root_fut = tokio::spawn(root::root(config.clone(), env.clone()));
    let web_fut = tokio::spawn(web::web(config.clone(), env.clone()));
    let node_fut = node::node(config.clone(), env.clone());

    // Run them in parallel until both finish
    // todo: this will only print errors at the end of execution
    let (root_res, web_res, node_res) = join!(root_fut, web_fut, node_fut);
    root_res??;
    web_res??;
    node_res?;

    return Ok(());
}
