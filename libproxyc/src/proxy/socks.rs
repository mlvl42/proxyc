use super::Proxy;
use crate::core::CONFIG;
use crate::error::Error;
use crate::util::read_timeout;
use byteorder::{BigEndian, WriteBytesExt};
use nix::unistd::write;
use proxyc_common::ProxyConf;
use std::io;
use std::io::Write;
use std::os::unix::io::RawFd;

pub struct Socks4;
pub struct Socks5;

// TODO:
// - create specific socks error
// - stop using global CONFIG

impl Proxy for Socks4 {
    type E = Error;

    fn connect(sock: RawFd, target: &ProxyConf) -> Result<(), Self::E> {
        let config = &*CONFIG;
        let mut packet = vec![];

        let _ = packet.write_u8(4); // version
        let _ = packet.write_u8(1); // connect

        // TODO handle auth and proxy dns

        match target.ip {
            std::net::IpAddr::V4(addr) => {
                packet.write_u16::<BigEndian>(target.port)?;
                packet.write_u32::<BigEndian>(addr.into())?;
                // write user here
                packet.write_u8(0)?;
            }
            _ => {
                return Err(Error::Generic(
                    "address family not supported by socks4".into(),
                ))
            }
        }

        write(sock, &packet)?;

        let mut buf = [0; 8];
        read_timeout(sock, &mut buf, config.tcp_read_timeout)?;

        if buf[0] != 0 {
            return Err(
                io::Error::new(io::ErrorKind::InvalidData, "invalid response version").into(),
            );
        }

        match buf[1] {
            90 => {}
            91 => {
                return Err(
                    io::Error::new(io::ErrorKind::Other, "request rejected or failed").into(),
                )
            }
            92 => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "request rejected because SOCKS server cannot connect to \
                                       identd on the client",
                )
                .into())
            }
            93 => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "request rejected because the client program and identd \
                                       report different user-ids",
                )
                .into())
            }
            _ => {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidData, "invalid response code").into(),
                )
            }
        }

        Ok(())
    }
}

fn write_addr(mut packet: &mut [u8], target: &ProxyConf) -> Result<usize, Error> {
    let start_len = packet.len();
    match target.ip {
        std::net::IpAddr::V4(addr) => {
            packet.write_u8(1).unwrap();
            packet.write_u32::<BigEndian>(addr.into()).unwrap();
            packet.write_u16::<BigEndian>(target.port).unwrap();
        }
        std::net::IpAddr::V6(addr) => {
            packet.write_u8(4).unwrap();
            packet.write_all(&addr.octets()).unwrap();
            packet.write_u16::<BigEndian>(target.port).unwrap();
        }
    }
    Ok(start_len - packet.len())
}

fn read_response(sock: RawFd) -> Result<(), Error> {
    let mut buf = [0; 4];
    let config = &*CONFIG;
    read_timeout(sock, &mut buf, config.tcp_read_timeout)?;

    if buf[0] != 5 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid response version").into());
    }

    match buf[1] {
        0 => {}
        1 => {
            return Err(io::Error::new(io::ErrorKind::Other, "general SOCKS server failure").into())
        }
        2 => {
            return Err(
                io::Error::new(io::ErrorKind::Other, "connection not allowed by ruleset").into(),
            )
        }
        3 => return Err(io::Error::new(io::ErrorKind::Other, "network unreachable").into()),
        4 => return Err(io::Error::new(io::ErrorKind::Other, "host unreachable").into()),
        5 => return Err(io::Error::new(io::ErrorKind::Other, "connection refused").into()),
        6 => return Err(io::Error::new(io::ErrorKind::Other, "TTL expired").into()),
        7 => return Err(io::Error::new(io::ErrorKind::Other, "command not supported").into()),
        8 => return Err(io::Error::new(io::ErrorKind::Other, "address kind not supported").into()),
        _ => return Err(io::Error::new(io::ErrorKind::Other, "unknown error").into()),
    }

    if buf[2] != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid reserved byte").into());
    }

    // read addr
    let len = match buf[3] {
        1 => 4,
        4 => 16,
        _ => return Err(io::Error::new(io::ErrorKind::Other, "unsupported address type").into()),
    };

    let mut buf = vec![0; len + 2];
    read_timeout(sock, &mut buf, config.tcp_read_timeout)?;

    Ok(())
}

impl Proxy for Socks5 {
    type E = Error;

    fn connect(sock: RawFd, target: &ProxyConf) -> Result<(), Self::E> {
        let config = &*CONFIG;

        let packet = [
            5, // version
            1, // methods
            0, // no auth
        ];
        write(sock, &packet)?;

        let mut buf = [0; 2];
        read_timeout(sock, &mut buf, config.tcp_read_timeout)?;

        let response_version = buf[0];
        let selected_method = buf[1];

        if response_version != 5 {
            return Err(
                io::Error::new(io::ErrorKind::InvalidData, "invalid response version").into(),
            );
        }

        if selected_method == 0xff {
            return Err(io::Error::new(io::ErrorKind::Other, "no acceptable auth method").into());
        }

        let mut packet = [0; 264];
        packet[0] = 5; // protocol version
        packet[1] = 1; // connect
        packet[2] = 0; // reserved

        // write address
        let len = write_addr(&mut packet[3..], target)?;
        write(sock, &packet[..len + 3])?;

        // read response + address on success
        read_response(sock)?;

        Ok(())
    }
}
