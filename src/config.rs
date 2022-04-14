// mirra (c) Nikolas Wipper 2022

use std::collections::HashMap;
use std::env;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use toml::value::Table;

use crate::util::{simple_input, simple_input_default};

#[derive(Debug)]
pub struct RootShare {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct RootSync {
    pub ip: String,
    pub port: u16,
    pub path: String,
}

#[derive(Debug)]
pub enum Root {
    Share(RootShare),
    Sync(RootSync),
}

#[derive(Debug)]
pub struct Config {
    pub name: String,
    pub port: u16,
    pub shares: HashMap<String, RootShare>,
    pub syncs: HashMap<String, RootSync>,
}

pub async fn setup(into: PathBuf) -> Result<Config> {
    let name: String = simple_input("mirra name?")?;
    let port: u16 = simple_input_default("mirra port?", 6007)?;

    if !into.join(".mirra").exists() {
        fs::create_dir(into.join(".mirra")).await?;
    }

    let config = Config {
        name,
        port,
        shares: HashMap::new(),
        syncs: HashMap::new(),
    };

    let mut toml_data = toml::map::Map::new();
    toml_data.insert("name".to_string(), config.name.clone().into());
    toml_data.insert("port".to_string(), toml::Value::Integer(config.port as i64));

    let mut config_file = File::create(into.join(".mirra/Mirra.toml")).await?;
    config_file.write_all(toml::to_string(&toml_data).unwrap().as_bytes()).await?;

    Ok(config)
}

async fn parse_table(table: &Table, name: String) -> Result<Root> {
    if table.contains_key("ip") && table.contains_key("port") {
        let ip = table.get("ip").unwrap();
        let port = table.get("port").unwrap();
        let p = table.get("path");

        if !ip.is_str() || !port.is_integer() || (p.is_some() && !p.unwrap().is_str()) {
            Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"))
        } else {
            let path: String = if p.is_some() {
                p.unwrap().as_str().unwrap().to_string()
            } else {
                name
            };
            Ok(Root::Sync(RootSync {
                ip: ip.as_str().unwrap().to_string(),
                port: port.as_integer().unwrap() as u16,
                path,
            }))
        }
    } else if table.contains_key("path") {
        let path = table.get("path").unwrap();

        if !path.is_str() {
            Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"))
        } else {
            Ok(Root::Share(RootShare {
                path: path.as_str().unwrap().to_string()
            }))
        }
    } else {
        Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"))
    }
}

/// Load configuration file
async fn load_config(from: &Path) -> Result<Config> {
    let mut mirra_file = File::open(from).await?;
    let mut config_raw = String::with_capacity(128);
    mirra_file.read_to_string(&mut config_raw).await?;

    let c = config_raw.as_str().parse::<toml::Value>();
    if c.is_err() || !c.as_ref().unwrap().is_table() {
        return Err(Error::new(ErrorKind::InvalidData, "Config file is corrupted"));
    }

    let config_value = c.unwrap();
    let config = config_value.as_table().unwrap();
    let mut name = "no name".to_string();
    let mut port = 6007u16;
    let mut syncs = HashMap::new();
    let mut shares = HashMap::new();

    for value in config {
        if value.0 == &"name".to_string() && value.1.is_str() {
            name = value.1.as_str().unwrap().to_string();
        } else if value.0 == &"port".to_string() && value.1.is_integer() {
            port = value.1.as_integer().unwrap() as u16;
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
    if !mirra_file.exists() {
        setup(env::current_dir()?).await
    } else {
        load_config(mirra_file).await
    }
}
