// mirra (c) Nikolas Wipper 2022

use std::io::{Error, ErrorKind, Result};
use std::net::SocketAddr;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use tokio::net::{TcpListener, TcpStream};

use crate::packet::{Packet, PacketKind, ReadAny, WriteAny};

/// Thin layer above [tokio::net::TcpListener]
pub struct Server {
    listener: TcpListener,
}

impl Server {
    /// Bind a server to 0.0.0.0:port
    pub async fn new(port: u16) -> Result<Self> {
        Ok(Server {
            listener: TcpListener::bind(SocketAddr::new("0.0.0.0".parse().unwrap(), port)).await?
        })
    }

    /// Wait for a new connection and accept it
    pub async fn accept(&mut self) -> Result<Client> {
        let (socket, _) = self.listener.accept().await?;
        Ok(Client {
            stream: socket
        })
    }
}

pub struct Client {
    pub(crate) stream: TcpStream,
}

impl Client {
    /// Connect to a server at ip:port
    pub async fn new(ip: String, port: u16) -> Result<Self> {
        Ok(Client {
            stream: TcpStream::connect(SocketAddr::new(ip.parse().unwrap(), port)).await?
        })
    }

    pub async fn expect<T: Packet>(&mut self) -> Result<T>
        where TcpStream: ReadAny<T> {
        let id = self.stream.read_u8().await?;
        if id == T::KIND as u8 {
            self.stream.read_any().await
        } else {
            Err(Error::new(ErrorKind::InvalidData, "unexpected package"))
        }
    }

    pub async fn expect_file(&mut self, mut file: File) -> Result<usize> {
        let id = self.stream.read_u8().await?;
        if id != PacketKind::File as u8 {
            return Err(Error::new(ErrorKind::InvalidData, "unexpected package"));
        }

        let mut size = self.stream.read_u64().await?;

        let mut buf = vec![0; 0x1000];

        loop {
            let to_read = size.min(0x1000) as usize;

            buf.resize(to_read, 0);
            let read = self.stream.read(buf.as_mut_slice()).await?;
            if read == 0 {
                break;
            }
            size -= read as u64;
            file.write(&buf.as_slice()[0..to_read]).await?;
        }

        Ok(size as usize)
    }

    pub async fn send<T: Packet>(&mut self, data: T) -> Result<usize>
        where TcpStream: WriteAny<T> {
        self.stream.write_u8(T::KIND as u8).await?;
        Ok(self.stream.write_any(data).await? + 1)
    }

    /// Returns the local address that this stream is bound to.
    pub fn peer_addr(&self) -> SocketAddr {
        self.stream.peer_addr().unwrap()
    }

    /// Returns the local address that this stream is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.stream.local_addr().unwrap()
    }
}
