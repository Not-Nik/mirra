// mirra (c) Nikolas Wipper 2022

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

extern crate core;

use std::env;
use std::io::{Error, ErrorKind, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use tokio::join;
use clap::{Parser, Subcommand};
use dialoguer::Confirm;

use crate::config::{get_config, RootShare, RootSync, safe_config};
use crate::keys::{LocalKeys, get_keys};
use crate::socket::{Client, Server};
use crate::util::stringify;

mod keys;
mod socket;
mod util;
mod root;
mod node;
mod packet;
mod config;
mod web;

#[derive(Parser)]
#[clap(name = "mirra")]
#[clap(about = "A mirror management software", version = "0.1.0")]
struct Cli {
    #[clap(subcommand)]
    commands: Subcommands
}

#[derive(Subcommand)]
enum Subcommands {
    #[clap(about = "Run mirra normally")]
    Run,
    #[clap(arg_required_else_help = true)]
    Sync(Sync),
    #[clap(arg_required_else_help = true)]
    Share(Share),
}

#[derive(clap::Args)]
#[clap(about = "Sync a module from a remote mirra")]
struct Sync {
    #[clap(value_name = "ADDR[:PORT]", help = "Set the remote mirra's address")]
    remote_addr: String,

    #[clap(help = "Set the remote module's name")]
    module: String,

    #[clap(short = 'p', long, parse(from_os_str), help = "Set where the module will be stored")]
    output_path: Option<PathBuf>,
}

#[derive(clap::Args)]
#[clap(about = "Share a local module to the interwebz")]
struct Share {
    #[clap(help = "Set the module's name")]
    name: String,

    #[clap(short = 'p', long, parse(from_os_str), help = "Set what directory to share")]
    module_path: Option<PathBuf>,
}

fn parse_addr(addr: String) -> Result<SocketAddr> {
    let app = if !addr.contains(":") {
        ":6007"
    } else {
        ""
    };
    let addr = SocketAddr::from_str(&(addr + app));
    if addr.is_ok() {
        Ok(addr.unwrap())
    } else {
        Err(Error::new(ErrorKind::AddrNotAvailable, addr.err().unwrap().to_string()))
    }
}

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
    let mut raw_config = get_config().await?;
    let raw_env = get_keys()?;

    let args = Cli::parse();

    match args.commands {
        Subcommands::Run => {
            let config = Arc::from(raw_config);
            let env = Arc::from(raw_env);

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
        },
        Subcommands::Sync(sync) => {
            if !raw_config.syncs.contains_key(&sync.module) ||
                Confirm::new()
                    .with_prompt(format!("Already syncing a module named {}. Overwrite?", sync.module))
                    .interact()? {
                let addr = parse_addr(sync.remote_addr)?;
                let path = if sync.output_path.is_some() {
                    stringify(sync.output_path.unwrap())?
                } else {
                    sync.module.as_str().to_string()
                };

                raw_config.syncs.insert(sync.module.clone(), RootSync {
                    ip: addr.ip().to_string(),
                    port: addr.port(),
                    path
                });
                safe_config(env::current_dir()?, raw_config).await?;
            }
        },
        Subcommands::Share(share) => {
            if !raw_config.shares.contains_key(&share.name) ||
                Confirm::new()
                    .with_prompt(format!("Already sharing a module named {}. Overwrite?", share.name))
                    .interact()? {
                let path = if share.module_path.is_some() {
                    stringify(share.module_path.unwrap())?
                } else {
                    share.name.as_str().to_string()
                };

                raw_config.shares.insert(share.name, RootShare {
                    path
                });
                safe_config(env::current_dir()?, raw_config).await?;
            }
        },
    }

    return Ok(());
}
