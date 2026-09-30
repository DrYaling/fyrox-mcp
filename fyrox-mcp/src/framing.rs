//! Strict Unity MCP framing: an unsigned 64-bit big-endian byte length.
use std::{
    io::{self, Read, Write},
    net::TcpStream,
};

pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub const HANDSHAKE: &[u8] = b"WELCOME UNITY-MCP 1 FRAMING=1\n";

pub fn read(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header)?;
    let length = u64::from_be_bytes(header);
    if length == 0 || length > MAX_FRAME_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid framed length",
        ));
    }
    let mut payload = vec![0; length as usize];
    stream.read_exact(&mut payload)?;
    Ok(payload)
}

pub fn write(stream: &mut TcpStream, payload: &[u8]) -> io::Result<()> {
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid framed length",
        ));
    }
    stream.write_all(&(payload.len() as u64).to_be_bytes())?;
    stream.write_all(payload)
}
