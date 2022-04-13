// mirra (c) Nikolas Wipper 2022

use std::fs;
use std::fs::{create_dir, File};
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::net::IpAddr;
use std::path::Path;

use log::error;
use rsa::{PaddingScheme, RsaPrivateKey, RsaPublicKey};
use rsa::pkcs1::LineEnding;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use serde::{Deserialize, Serialize};

use crate::util::{simple_input, simple_input_default};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    pub port: u16,
    pub is_root: bool,
    pub node_config: Option<NodeConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct NodeConfig {
    pub root_addr: String,
    pub root_port: u16,
}

pub struct Environment {
    pub private_key: rsa::RsaPrivateKey,
    pub public_key: rsa::RsaPublicKey,
}

impl Environment {
    pub fn sign(&self, msg: String) -> String {
        base64::encode(self.private_key.sign(PaddingScheme::PKCS1v15Sign { hash: None }, msg.as_bytes()).unwrap())
    }
}

/// Generate private and public key and store them to disk
fn setup_environment(at: &Path) -> Result<Environment> {
    let mut rng = rand::thread_rng();

    let bits = 2048;
    let private_key = rsa::RsaPrivateKey::new(&mut rng, bits).expect("failed to generate a key");
    let public_key = rsa::RsaPublicKey::from(&private_key);

    let encoded_priv = private_key.to_pkcs8_pem(LineEnding::LF).expect("failed to encode a key");
    let encoded_pub = public_key.to_public_key_pem(LineEnding::LF).expect("failed to encode a key");

    let mut private_key_file = File::create(at.join("private.key"))?;
    let mut public_key_file = File::create(at.join("public.key"))?;

    private_key_file.write_all(encoded_priv.as_bytes())?;
    public_key_file.write_all(encoded_pub.as_bytes())?;

    Ok(Environment {
        private_key,
        public_key,
    })
}

/// Delete both keys if they exist
fn clear_keys(at: &Path) -> Result<()> {
    if at.join("private.key").exists() {
        fs::remove_file(at.join("private.key"))?;
    }

    if at.join("public.key").exists() {
        fs::remove_file(at.join("public.key"))?;
    }

    Ok(())
}

/// Load only the private key from disk
fn load_private_key(from: &Path) -> Result<RsaPrivateKey> {
    let mut private_key_file = File::open(from.join("private.key"))?;
    let mut encoded_priv = String::with_capacity(512);
    private_key_file.read_to_string(&mut encoded_priv)?;
    let private_key = RsaPrivateKey::from_pkcs8_pem(encoded_priv.as_str());

    if private_key.is_err() {
        Err(Error::new(ErrorKind::InvalidData, "failed to load a key"))
    } else {
        Ok(private_key.unwrap())
    }
}

/// Load only the public key from disk
fn load_public_key(from: &Path) -> Result<RsaPublicKey> {
    let mut public_key_file = File::open(from.join("public.key"))?;
    let mut encoded_pub = String::with_capacity(512);
    public_key_file.read_to_string(&mut encoded_pub)?;
    let public_key = RsaPublicKey::from_public_key_pem(encoded_pub.as_str());

    if public_key.is_err() {
        Err(Error::new(ErrorKind::InvalidData, "failed to load a key"))
    } else {
        Ok(public_key.unwrap())
    }
}

/// Load both keys from disk, regenerate if they don't exist
fn load_environment(from: &Path) -> Result<Environment> {
    let private_key = load_private_key(from);

    if private_key.is_err() {
        error!("Mirra was configured, but private key is missing, regenerating...");
        clear_keys(from)?;
        return setup_environment(from);
    }

    let mut public_key = load_public_key(from);

    if public_key.is_err() {
        let public_key_file_path = from.join("public.key");
        if public_key_file_path.exists() { fs::remove_file(public_key_file_path.clone())?; }
        public_key = Ok(private_key.as_ref().unwrap().to_public_key());

        let encoded_pub = public_key.as_ref().unwrap().to_public_key_pem(LineEnding::LF).expect("failed to encode a key");

        let mut public_key_file = File::create(public_key_file_path.clone())?;

        public_key_file.write_all(encoded_pub.as_bytes())?;
    }

    Ok(Environment {
        private_key: private_key.unwrap(),
        public_key: public_key.unwrap(),
    })
}

/// Abstraction for loading/creating private/public keys
pub fn get_environment() -> Result<Environment> {
    let mirra_folder = Path::new(".mirra");
    if !mirra_folder.exists() {
        create_dir(mirra_folder)?;
    }
    load_environment(mirra_folder)
}

/// Create configuration file
fn setup_config(from: &Path) -> Result<Config> {
    let name: String = simple_input("mirra name?")?;
    let port: u16 = simple_input_default("mirra port?", 6007)?;
    let is_root: bool = simple_input_default("is this a root mirra?", false)?;

    let mut config = Config {
        name,
        port,
        is_root,
        node_config: None,
    };

    if !is_root {
        let root_addr: IpAddr = simple_input("root mirra's ip?")?;
        let root_port: u16 = simple_input_default("root mirra's port?", 6007)?;

        config.node_config = Some(NodeConfig {
            root_addr: root_addr.to_string(),
            root_port,
        });
    }

    let mut config_file = File::create(from.join("Mirra.toml"))?;
    config_file.write_all(toml::to_string(&config).unwrap().as_bytes())?;

    return Ok(config);
}

/// Load configuration file
fn load_config(from: &Path) -> Result<Config> {
    let config_file_path = from.join("Mirra.toml");
    if !config_file_path.exists() {
        return setup_config(from);
    }

    let mut config_file = File::open(config_file_path)?;
    let mut config_raw = String::with_capacity(512);
    config_file.read_to_string(&mut config_raw)?;

    let config: Config = toml::from_str(config_raw.as_str())?;
    Ok(config)
}

/// Abstraction for loading/creating the configuration file
pub fn get_config() -> Result<Config> {
    let mirra_folder = Path::new(".mirra");
    if !mirra_folder.exists() {
        create_dir(mirra_folder)?;
    }
    load_config(mirra_folder)
}
