// mirra (c) Nikolas Wipper 2022

use std::io::{Error, ErrorKind, Result};
use std::path::PathBuf;

use async_trait::async_trait;
use log::warn;
use num_derive::FromPrimitive;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(PartialEq, FromPrimitive)]
pub enum PacketKind {
    Ok = 0x1,
    Close = 0x2,
    Handshake = 0x3,
    Sync = 0x4,
    Continue = 0x5,
    FileHeader = 0x6,
    File = 0x7,
    Skip = 0x8
}

pub trait Packet {
    const KIND: PacketKind;
}

#[async_trait]
pub trait WriteAny<T> {
    async fn write_any(&mut self, t: T) -> Result<usize>;
}

#[async_trait]
pub trait ReadAny<T> {
    async fn read_any(&mut self) -> Result<T>;
}

impl Packet for File {
    const KIND: PacketKind = PacketKind::File;
}

impl Packet for PathBuf {
    const KIND: PacketKind = PacketKind::File;
}

#[async_trait]
impl WriteAny<String> for TcpStream {
    async fn write_any(&mut self, t: String) -> Result<usize> {
        self.write_u32(t.len() as u32).await?;
        self.write(t.as_bytes()).await
    }
}

#[async_trait]
impl ReadAny<String> for TcpStream {
    async fn read_any(&mut self) -> Result<String> {
        let size = self.read_u32().await? as usize;
        let mut buf = vec![0; size];
        self.read_exact(buf.as_mut_slice()).await?;
        let res = String::from_utf8(buf);
        if res.is_ok() {
            Ok(res.unwrap())
        } else {
            Err(Error::new(ErrorKind::InvalidData, "couldn't decode utf8"))
        }
    }
}

#[async_trait]
impl WriteAny<File> for TcpStream {
    async fn write_any(&mut self, mut t: File) -> Result<usize> {
        let expected = t.metadata().await?.len();
        self.write_u64(expected).await?;
        let mut read = 0;
        let mut written = 0;

        let mut buf = vec![0; 0x1000];
        loop {

            let s = t.read(buf.as_mut_slice()).await?;

            if s == 0 {
                break;
            }
            read += s;

            written += self.write(&buf.as_slice()[0..s]).await?;
        }

        if read != written {
            warn!("Read {} bytes from disk, but sent only {} via network", read, written)
        }
        if written != expected as usize {
            warn!("Announced to send {} bytes, but sent {}", written, expected);
        }

        Ok(written)
    }
}

#[async_trait]
impl WriteAny<Handshake> for TcpStream {
    async fn write_any(&mut self, t: Handshake) -> Result<usize> {
        Ok(self.write_any(t.name).await?)
    }
}

#[async_trait]
impl ReadAny<Handshake> for TcpStream {
    async fn read_any(&mut self) -> Result<Handshake> {
        Ok(Handshake {
            name: self.read_any().await?,
        })
    }
}

#[async_trait]
impl WriteAny<ContinueSync> for TcpStream {
    async fn write_any(&mut self, t: ContinueSync) -> Result<usize> {
        self.write_u8(t.cont as u8).await?;
        Ok(1)
    }
}

#[async_trait]
impl ReadAny<ContinueSync> for TcpStream {
    async fn read_any(&mut self) -> Result<ContinueSync> {
        Ok(ContinueSync { cont: self.read_u8().await? != 0 })
    }
}

#[async_trait]
impl WriteAny<FileHeader> for TcpStream {
    async fn write_any(&mut self, t: FileHeader) -> Result<usize> {
        Ok(self.write_any(t.path).await? + self.write_any(t.hash).await? + self.write_any(t.cert).await?)
    }
}

#[async_trait]
impl ReadAny<FileHeader> for TcpStream {
    async fn read_any(&mut self) -> Result<FileHeader> {
        Ok(FileHeader {
            path: self.read_any().await?,
            hash: self.read_any().await?,
            cert: self.read_any().await?,
        })
    }
}

macro_rules! generic_packet {
    ($name:ident, $id:expr) => {
        pub struct $name {}
        impl $name { pub fn new() -> Self { Self {} } }
        impl Packet for $name { const KIND: PacketKind = $id; }
        #[async_trait]
        impl WriteAny<$name> for TcpStream { async fn write_any(&mut self, _t: $name) -> Result<usize> { Ok(0) } }

        #[async_trait]
        impl ReadAny<$name> for TcpStream { async fn read_any(&mut self) -> Result<$name> { Ok($name {}) } }
    };
    ($name:ident, $id:expr, $($arg:ident, $typ:ty),*) => {
        pub struct $name {
            $(
                pub $arg: $typ,
            )*
        }

        impl $name {
            pub fn new(
                $(
                    $arg: $typ,
                )*
            ) -> Self {
                Self {
                    $(
                        $arg,
                    )*
                }
            }
        }

        impl Packet for $name { const KIND: PacketKind = $id; }
    }
}

generic_packet!(Ok, PacketKind::Ok);
generic_packet!(Close, PacketKind::Close);
generic_packet!(Handshake, PacketKind::Handshake, name, String);
generic_packet!(Sync, PacketKind::Sync);
generic_packet!(ContinueSync, PacketKind::Continue, cont, bool);
generic_packet!(FileHeader, PacketKind::FileHeader, path, String, hash, String, cert, String);
generic_packet!(Skip, PacketKind::Skip);
