// mirra (c) Nikolas Wipper 2022

use std::io::{Error, ErrorKind, Result};
use std::net::SocketAddr;
use log::warn;
use num_traits::FromPrimitive;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use tokio::net::{TcpListener, TcpStream};

use crate::packet::{Close, Packet, PacketKind, ReadAny, WriteAny};

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

/// Thin layer above [tokio::net::TcpStream]
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

    /// Only read a packets id
    pub async fn read_packet_kind(&mut self) -> Result<PacketKind> {
        let t = self.stream.read_u8().await?;
        let res = FromPrimitive::from_u8(t);

        if res.is_some() {
            Ok(res.unwrap())
        } else {
            Err(Error::new(ErrorKind::InvalidData, "invalid packet kind"))
        }
    }

    /// Read a packet without reading its kind
    pub async fn expect_unchecked<T>(&mut self) -> Result<T>
        where TcpStream: ReadAny<T> {
        self.stream.read_any().await
    }

    /// Read a packet
    pub async fn expect<T: Packet>(&mut self) -> Result<T>
        where TcpStream: ReadAny<T> {
        let id = self.read_packet_kind().await?;
        if id == T::KIND {
            Ok(self.expect_unchecked().await?)
        } else {
            Err(Error::new(ErrorKind::InvalidData, "unexpected package"))
        }
    }

    /// Read a file, as if a file was a packet with kind [PacketKind::File], and write to [file]
    pub async fn expect_file(&mut self, mut file: File) -> Result<usize> {
        let id = self.stream.read_u8().await?;
        if id != PacketKind::File as u8 {
            return Err(Error::new(ErrorKind::InvalidData, "unexpected package"));
        }

        // Get the size of the file
        let mut size = self.stream.read_u64().await?;

        // Assuming a good size of 0x1000, because that's likely to be one page in memory
        let mut buf = vec![0; 0x1000];

        loop {
            // Read 0x1000 at max
            let to_read = size.min(0x1000) as usize;

            buf.truncate(to_read);
            // Read from remote host
            let read = self.stream.read(buf.as_mut_slice()).await?;
            if read == 0 {
                break;
            }
            size -= read as u64;
            // Write to file
            file.write(&buf.as_slice()[0..to_read]).await?;
        }

        Ok(size as usize)
    }

    /// Write a packet
    pub async fn send<T: Packet>(&mut self, data: T) -> Result<usize>
        where TcpStream: WriteAny<T> {
        self.stream.write_u8(T::KIND as u8).await?;
        Ok(self.stream.write_any(data).await? + 1)
    }

    /// Write a file, as if a file was a packet with kind [PacketKind::File]
    /// This assumes [file] to be locked, or not to be changed during sending
    pub async fn send_file(&mut self, file: &mut File) -> Result<usize> {
        // Write the packet kind
        self.stream.write_u8(PacketKind::File as u8).await?;

        let expected = file.metadata().await?.len();
        // Write the size
        self.stream.write_u64(expected).await?;

        // Safety counters
        let mut read = 0;
        let mut written = 0;

        // Again, 0x1000 is likely the size of a page
        let mut buf = vec![0; 0x1000];
        loop {
            // Read from file
            let s = file.read(buf.as_mut_slice()).await?;

            if s == 0 {
                break;
            }
            read += s;

            // Write to remote host
            written += self.stream.write(&buf.as_slice()[0..s]).await?;
        }
        if read != written {
            warn!("Read {} bytes from disk, but sent only {} via network", read, written)
        }
        if written != expected as usize {
            warn!("Announced to send {} bytes, but sent {}", expected, written);
        }

        Ok(written)
    }

    /// Close the connection (from the nodes perspective)
    pub async fn close(&mut self) -> Result<()> {
        self.send(Close::new()).await?;
        self.expect::<Close>().await?;
        Ok(())
    }

    /// Returns the local address that this stream is bound to.
    pub fn peer_addr(&self) -> SocketAddr {
        self.stream.peer_addr().unwrap()
    }
}
