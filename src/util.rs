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

/// Gets an input of type [T] with a prompt
pub fn simple_input<S: Into<String>, T>(prompt: S) -> Result<T>
    where
        T: Clone + ToString + FromStr,
        <T as FromStr>::Err: Debug + ToString {
    Input::new()
        .with_prompt(prompt)
        .interact_text()
}

/// Gets an input of type [T] with a prompt and a default value
pub fn simple_input_default<S: Into<String>, T>(prompt: S, default: T) -> Result<T>
    where
        T: Clone + ToString + FromStr,
        <T as FromStr>::Err: Debug + ToString {
    Input::new()
        .with_prompt(prompt)
        .default(default)
        .interact_text()
}

/// Returns a path as an optional string
pub fn stringify(path: impl AsRef<Path>) -> Result<String> {
    let str = path.as_ref().to_str();
    if str.is_none() {
        return Err(Error::new(ErrorKind::Other, "failed to decode path"));
    }
    Ok(str.unwrap().to_string())
}

/// Returns the hash of a files contents
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
    // Seek back to start to make file usable again
    // Doesn't have to save state before, because its only
    // ever called directly after opening a file
    file.seek(SeekFrom::Start(0)).await?;

    Ok(hasher.finalize().to_string())
}

/// Convenience trait for locking and unlocking a file asynchronously
#[async_trait]
pub trait AsyncFileLock {
    /// Lock a file
    async fn lock(&self) -> Result<()>;
    /// Unlock a file
    async fn unlock(&self) -> Result<()>;
}

#[async_trait]
impl AsyncFileLock for File {
    async fn lock(&self) -> Result<()> {
        // Todo: maybe do this with [fs4::tokio::AsyncFileExt::try_lock_exclusive]
        // Local copy for the thread
        let copy = self.try_clone().await?;
        // Do the blocking stuff in a thread
        match tokio::task::spawn_blocking(move || copy.lock_exclusive()).await {
            Ok(res) => res,
            Err(_) => Err(Error::new(
                ErrorKind::Other,
                "background task failed",
            )),
        }
    }

    async fn unlock(&self) -> Result<()> {
        // Local copy for the thread
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
