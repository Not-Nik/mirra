// mirra (c) Nikolas Wipper 2022

use std::fmt::Debug;
use std::io::{Error, ErrorKind, Result, SeekFrom, Write};
use std::path::Path;
use std::str::FromStr;
use blake3::Hasher;

use async_trait::async_trait;
use dialoguer::Input;
use fs4::tokio::AsyncFileExt;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub fn simple_input<S: Into<String>, T>(prompt: S) -> Result<T>
    where
        T: Clone + ToString + FromStr,
        <T as FromStr>::Err: Debug + ToString {
    Input::new()
        .with_prompt(prompt)
        .interact_text()
}

pub fn simple_input_default<S: Into<String>, T>(prompt: S, default: T) -> Result<T>
    where
        T: Clone + ToString + FromStr,
        <T as FromStr>::Err: Debug + ToString {
    Input::new()
        .with_prompt(prompt)
        .default(default)
        .interact_text()
}

pub fn stringify(path: impl AsRef<Path>) -> Result<String> {
    let str = path.as_ref().to_str();
    if str.is_none() {
        return Err(Error::new(ErrorKind::Other, "failed to decode path"));
    }
    Ok(str.unwrap().to_string())
}

pub async fn hash_file(file: &mut File) -> Result<String> {
    let mut buf = vec![0; 0x1000];
    let mut hasher = Hasher::new();
    loop {
        let s = file.read(buf.as_mut_slice()).await?;
        if s == 0 {
            break;
        }

        hasher.write(&buf.as_slice()[0..s])?;
    }
    file.seek(SeekFrom::Start(0)).await?;

    Ok(hasher.finalize().to_string())
}

#[async_trait]
pub trait AsyncFileLock {
    async fn lock(&self) -> Result<()>;
    async fn unlock(&self) -> Result<()>;
}

#[async_trait]
impl AsyncFileLock for File {
    async fn lock(&self) -> Result<()> {
        let copy = self.try_clone().await?;
        match tokio::task::spawn_blocking(move || copy.lock_exclusive()).await {
            Ok(res) => res,
            Err(_) => Err(Error::new(
                ErrorKind::Other,
                "background task failed",
            )),
        }
    }

    async fn unlock(&self) -> Result<()> {
        let copy = self.try_clone().await?;
        match tokio::task::spawn_blocking(move || AsyncFileExt::unlock(&copy)).await {
            Ok(res) => res,
            Err(_) => Err(Error::new(
                ErrorKind::Other,
                "background task failed",
            )),
        }
    }
}
