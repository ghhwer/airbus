use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};

pub struct BoundListener {
    listener: TcpListener,
    host: String,
    port: u16,
}

impl BoundListener {
    pub fn bind(host: &str, port: u16) -> io::Result<Self> {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(false)?;
        let bound = listener.local_addr()?;
        Ok(Self {
            listener,
            host: host.to_string(),
            port: bound.port(),
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn accept(&self) -> io::Result<TcpStream> {
        let (stream, _) = self.listener.accept()?;
        Ok(stream)
    }
}

pub fn recv_all(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    stream.read_to_end(&mut out)?;
    Ok(out)
}

pub fn send_all(stream: &mut TcpStream, data: &[u8]) -> io::Result<()> {
    stream.write_all(data)?;
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}
