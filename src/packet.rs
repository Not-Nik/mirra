// mirra (c) Nikolas Wipper 2022

use std::io::{Error, ErrorKind, Result};
use std::path::PathBuf;

use async_trait::async_trait;
use num_derive::FromPrimitive;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(PartialEq, FromPrimitive)]
pub enum PacketKind {
    Ok = 0x1,
    Close = 0x2,
    Handshake = 0x3,
    Index = 0x4,
    Sync = 0x5,
    Continue = 0x6,
    FileHeader = 0x7,
    File = 0x8,
    Skip = 0x9,
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
impl WriteAny<bool> for TcpStream {
    async fn write_any(&mut self, t: bool) -> Result<usize> {
        self.write_u8(t as u8).await?;
        Ok(1)
    }
}

#[async_trait]
impl ReadAny<bool> for TcpStream {
    async fn read_any(&mut self) -> Result<bool> {
        Ok(self.read_u8().await? != 0)
    }
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
impl WriteAny<Vec<String>> for TcpStream {
    async fn write_any(&mut self, t: Vec<String>) -> Result<usize> {
        self.write_u32(t.len() as u32).await?;
        let mut written = 4;
        for el in t {
            written += self.write_any(el).await?;
        }
        Ok(written)
    }
}

#[async_trait]
impl ReadAny<Vec<String>> for TcpStream {
    async fn read_any(&mut self) -> Result<Vec<String>> {
        let size = self.read_u32().await? as usize;
        let mut res = Vec::with_capacity(size);
        for _ in 0..size {
            res.push(self.read_any().await?);
        }
        Ok(res)
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

        #[async_trait]
        impl WriteAny<$name> for TcpStream {
            async fn write_any(&mut self, t: $name) -> Result<usize> {
                Ok(
                    $(
                    self.write_any(t.$arg).await? +
                    )* 0
                )
            }
        }

        #[async_trait]
        impl ReadAny<$name> for TcpStream {
            async fn read_any(&mut self) -> Result<$name> {
                Ok($name {
                    $(
                    $arg: self.read_any().await?,
                    )*
                })
            }
        }
    }
}

generic_packet!(Ok, PacketKind::Ok);
generic_packet!(Close, PacketKind::Close);
generic_packet!(Handshake, PacketKind::Handshake, name, String);
generic_packet!(IndexNode, PacketKind::Index);
generic_packet!(IndexRoot, PacketKind::Index, modules, Vec<String>);
generic_packet!(Sync, PacketKind::Sync, module, String);
generic_packet!(ContinueSync, PacketKind::Continue, cont, bool);
generic_packet!(FileHeader, PacketKind::FileHeader, path, String, hash, String, cert, String);
generic_packet!(Skip, PacketKind::Skip);
