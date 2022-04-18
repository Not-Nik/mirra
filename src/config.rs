// mirra (c) Nikolas Wipper 2022

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::HashMap;
use std::env;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use toml::Value;
use toml::value::Table;

use crate::util::{simple_input, simple_input_default};

#[derive(Debug)]
/// Registers a root-only path to be synced over the network with nodes
pub struct RootShare {
    pub path: String,
}

#[derive(Debug, Clone)]
/// Registers a node that syncs from [ip]:[port] into [path]
pub struct RootSync {
    pub ip: String,
    pub port: u16,
    pub path: String,
}

#[derive(Debug)]
/// Convenience enum for parsing TOML config files
pub enum Root {
    Share(RootShare),
    Sync(RootSync),
}

#[derive(Debug)]
/// Holds information about the server instance and the modules it shares and syncs
pub struct Config {
    pub name: String,
    pub port: u16,
    pub shares: HashMap<String, RootShare>,
    pub syncs: HashMap<String, RootSync>,
}

/// Create a .mirra directory and .mirra/Mirra.toml file if they don't exist
pub async fn setup_config(into: PathBuf) -> Result<Config> {
    // Get basic info from user
    let name: String = simple_input("mirra name?")?;
    let port: u16 = simple_input_default("mirra port?", 6007)?;

    // Create config dir if it doesn't exist
    if !into.join(".mirra").exists() {
        fs::create_dir(into.join(".mirra")).await?;
    }

    let config = Config {
        name,
        port,
        shares: HashMap::new(),
        syncs: HashMap::new(),
    };

    // Put data into TOML format
    let mut toml_data = toml::map::Map::new();
    toml_data.insert("name".to_string(), config.name.clone().into());
    toml_data.insert("port".to_string(), toml::Value::Integer(config.port as i64));

    // [setup_config] is only called when .mirra/Mirra.toml doesn't exist so this is save
    // Save TOML config data to disk
    let mut config_file = File::create(into.join(".mirra/Mirra.toml")).await?;
    config_file.write_all(toml::to_string(&toml_data).unwrap().as_bytes()).await?;

    Ok(config)
}

/// Parse a TOML table from a Mirra.toml config file
async fn parse_table(table: &Table, name: String) -> Result<Root> {
    // Syncs need an ip and a port but not a path
    if table.contains_key("ip") && table.contains_key("port") {
        // Get values
        let ip = table.get("ip").unwrap();
        let port = table.get("port").unwrap();
        let p = table.get("path");

        // Check value validity
        if !ip.is_str() || !port.is_integer() || (p.is_some() && !p.unwrap().is_str()) {
            Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"))
        } else {
            // Glorified custom unwrap_or
            let path: String = if p.is_some() {
                p.unwrap().as_str().unwrap().to_string()
            } else {
                name
            };
            // Return sync object
            Ok(Root::Sync(RootSync {
                ip: ip.as_str().unwrap().to_string(),
                port: port.as_integer().unwrap() as u16,
                path,
            }))
        }
    // Shares need a path for now
    } else if table.contains_key("path") {
        // Get value
        let path = table.get("path").unwrap();

        // Check value validity
        if !path.is_str() {
            Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"))
        } else {
            // Return share object
            Ok(Root::Share(RootShare {
                path: path.as_str().unwrap().to_string()
            }))
        }
    // Tables that contain none of these, e.g. empty tables are invalid
    } else {
        Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"))
    }
}

/// Load a Mirra.toml configuration file
async fn load_config(from: &Path) -> Result<Config> {
    // Config file always exist when [load_config] is called
    // Load raw config data from disk
    let mut mirra_file = File::open(from).await?;
    let mut config_raw = String::with_capacity(128);
    mirra_file.read_to_string(&mut config_raw).await?;

    let c = config_raw.as_str().parse::<toml::Value>();
    if c.is_err() || !c.as_ref().unwrap().is_table() {
        return Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"));
    }

    // Create temporary value, because tables are always borrows
    let config_value = c.unwrap();
    let config = config_value.as_table().unwrap();

    // Default values
    let mut name = "no name".to_string();
    let mut port = 6007u16;
    let mut syncs = HashMap::new();
    let mut shares = HashMap::new();

    for value in config {
        // Any `name = "..."`
        if value.0 == &"name".to_string() && value.1.is_str() {
            name = value.1.as_str().unwrap().to_string();
        // Any `port = xxxx`
        } else if value.0 == &"port".to_string() && value.1.is_integer() {
            port = value.1.as_integer().unwrap() as u16;
        // Any `[table_name]\nxxx = xxx`
        } else if value.1.is_table() {
            let table = value.1.as_table().unwrap();
            let root = parse_table(table, value.0.clone()).await?;

            match root {
                Root::Share(share) => { shares.insert(value.0.clone(), share); }
                Root::Sync(sync) => { syncs.insert(value.0.clone(), sync); }
            }
        }
    }

    Ok(Config {
        name,
        port,
        shares,
        syncs,
    })
}

/// Abstraction for loading/creating the configuration file
pub async fn get_config() -> Result<Config> {
    let mirra_file = Path::new(".mirra/Mirra.toml");
    // Check if config exists, else create
    if !mirra_file.exists() {
        setup_config(env::current_dir()?).await
    } else {
        load_config(mirra_file).await
    }
}

pub async fn safe_config(into: PathBuf, config: Config) -> Result<()> {
    let mut toml_data = toml::map::Map::new();
    toml_data.insert("name".to_string(), config.name.clone().into());
    toml_data.insert("port".to_string(), toml::Value::Integer(config.port as i64));

    for share in config.shares {
        toml_data.insert(share.0, Value::Table(Table::from_iter([
            ("path".to_string(), Value::String(share.1.path))
        ].into_iter())));
    }

    for sync in config.syncs {
        toml_data.insert(sync.0, Value::Table(Table::from_iter([
            ("ip".to_string(), Value::String(sync.1.ip)),
            ("port".to_string(), Value::Integer(sync.1.port as i64)),
            ("path".to_string(), Value::String(sync.1.path))
        ].into_iter())));
    }

    let mut config_file = File::create(into.join(".mirra/Mirra.toml")).await?;
    config_file.write_all(toml::to_string(&toml_data).unwrap().as_bytes()).await?;

    Ok(())
}
