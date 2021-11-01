use super::Proxy;
use crate::core::CONFIG;
use crate::error::Error;
use crate::util::read_timeout;
use nix::unistd::write;
use proxyc_common::ProxyConf;
use std::io;
use std::os::unix::io::RawFd;

pub struct Http;

impl Proxy for Http {
    type E = Error;

    fn connect(sock: RawFd, target: &ProxyConf) -> Result<(), Self::E> {
        let config = &*CONFIG;
        let ip = match target.ip {
            std::net::IpAddr::V4(addr) => addr.to_string(),
            std::net::IpAddr::V6(addr) => addr.to_string(),
        };

        let packet = format!("CONNECT {}:{} HTTP/1.0\r\n\r\n", ip, target.port);
        let packet = packet.as_bytes();
        write(sock, packet)?;

        let mut len = 0;
        let mut buf = [0; 1024];
        while len < 1024 {
            read_timeout(sock, &mut buf[len..len + 1], config.tcp_read_timeout)?;
            len += 1;
            if len > 4
                && (buf[len - 1] == b'\n'
                    && buf[len - 2] == b'\r'
                    && buf[len - 3] == b'\n'
                    && buf[len - 4] == b'\r')
            {
                break;
            }
        }

        if len == 1024 || !(buf[9] == b'2' && buf[10] == b'0' && buf[11] == b'0') {
            return Err(io::Error::new(io::ErrorKind::Other, "HTTP proxy blocked").into());
        }

        Ok(())
    }
}
