// mirra (c) Nikolas Wipper 2022

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::io::{Error, ErrorKind, Result};

use async_trait::async_trait;
use num_derive::FromPrimitive;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(PartialEq, FromPrimitive)]
pub enum PacketKind {
    Ok = 0x1,
    Close = 0x2,
    Handshake = 0x3,
    NotFound = 0x4,
    Heartbeat = 0x5,
    BeginSync = 0x6,
    EndSync = 0x7,
    FileHeader = 0x8,
    File = 0x9,
    Remove = 0xA,
    Rename = 0xB,
    Skip = 0xC,
}

/// Convenience trait for passing [PacketKinds]'s
pub trait Packet {
    const KIND: PacketKind;
}

/// Convenience trait for writing to TcpStream
#[async_trait]
pub trait WriteAny<T> {
    /// Write [t] to the stream
    async fn write_any(&mut self, t: T) -> Result<usize>;
}

/// Convenience trait for reading from TcpStream
#[async_trait]
pub trait ReadAny<T> {
    /// Read a [T] from the stream
    async fn read_any(&mut self) -> Result<T>;
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
        // Encoding is 4 bytes of size, then the entire string as utf8
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
        // Again, 4 bytes of len, then every element
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
generic_packet!(Handshake, PacketKind::Handshake, module, String);
generic_packet!(NotFound, PacketKind::NotFound);
generic_packet!(Heartbeat, PacketKind::Heartbeat);
generic_packet!(BeginSync, PacketKind::BeginSync);
generic_packet!(EndSync, PacketKind::EndSync);
generic_packet!(FileHeader, PacketKind::FileHeader, path, String, hash, String, cert, String);
generic_packet!(Remove, PacketKind::Remove, path, String);
generic_packet!(Rename, PacketKind::Rename, old, String, new, String);
generic_packet!(Skip, PacketKind::Skip);
