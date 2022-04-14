// mirra (c) Nikolas Wipper 2022

use std::fs;
use std::fs::{create_dir, File};
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::path::Path;

use log::error;
use rsa::{PaddingScheme, RsaPrivateKey, RsaPublicKey};
use rsa::pkcs1::LineEnding;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};

pub struct LocalKeys {
    pub private_key: rsa::RsaPrivateKey,
    pub public_key: rsa::RsaPublicKey,
}

impl LocalKeys {
    pub fn sign(&self, msg: String) -> String {
        base64::encode(self.private_key.sign(PaddingScheme::PKCS1v15Sign { hash: None }, msg.as_bytes()).unwrap())
    }
}

/// Generate private and public key and store them to disk
fn setup_environment(at: &Path) -> Result<LocalKeys> {
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

    Ok(LocalKeys {
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
fn load_environment(from: &Path) -> Result<LocalKeys> {
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

    Ok(LocalKeys {
        private_key: private_key.unwrap(),
        public_key: public_key.unwrap(),
    })
}

/// Abstraction for loading/creating private/public keys
pub fn get_environment() -> Result<LocalKeys> {
    let mirra_folder = Path::new(".mirra");
    if !mirra_folder.exists() {
        create_dir(mirra_folder)?;
    }
    load_environment(mirra_folder)
}
