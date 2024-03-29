use super::Proxy;
use crate::core::{CONFIG, INTERNALADDR};
use crate::error::Error;
use crate::util::read_timeout;
use byteorder::{BigEndian, WriteBytesExt};
use nix::unistd::write;
use proxyc_common::{Auth, ProxyConf};
use std::io;
use std::io::Write;
use std::net::IpAddr;
use std::os::unix::io::RawFd;

pub struct Socks4;
pub struct Socks5;

impl Proxy for Socks4 {
    type E = Error;

    fn connect(sock: RawFd, target: &ProxyConf, _auth: Option<&Auth>) -> Result<(), Self::E> {
        let config = &*CONFIG;
        let mut packet = vec![];

        let _ = packet.write_u8(4); // version
        let _ = packet.write_u8(1); // connect

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

fn write_hostname(mut packet: &mut [u8], target: &ProxyConf, hn: String) -> Result<usize, Error> {
    let start_len = packet.len();
    let hn_len: u8 = hn.len().try_into().unwrap();
    packet.write_u8(3).unwrap(); // dns
    packet.write_u8(hn_len).unwrap();
    packet.write_all(hn.as_bytes()).unwrap();
    packet.write_u16::<BigEndian>(target.port).unwrap();
    Ok(start_len - packet.len())
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

impl Socks5 {
    fn auth_id(auth: Option<&Auth>) -> u8 {
        match auth {
            Some(Auth::UserPassword { .. }) => 2,
            None => 0,
        }
    }
}

fn find_ip_hostname(ip: IpAddr) -> Option<String> {
    let config = &*CONFIG;

    if !config.proxy_dns {
        return None;
    }

    let internal_addr = &mut *INTERNALADDR.lock().expect("mutex poisoned");
    if let std::net::IpAddr::V4(addr) = ip {
        let parts = addr.octets();
        let idx: u32 = addr.into();
        if parts[0] == config.dns_subnet {
            return internal_addr.get_hostname(idx).ok();
        }
    }
    None
}

impl Proxy for Socks5 {
    type E = Error;

    fn authenticate(sock: RawFd, auth: Option<&Auth>) -> Result<(), Self::E> {
        if let Some(Auth::UserPassword(user, password)) = auth {
            let config = &*CONFIG;
            if user.is_empty() || user.len() > 255 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid username").into());
            };
            if password.is_empty() || password.len() > 255 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid password").into());
            }

            let mut packet = [0; 515];
            let packet_size = 3 + user.len() + password.len();
            packet[0] = 1; // version
            packet[1] = user.len() as u8;
            packet[2..2 + user.len()].copy_from_slice(user.as_bytes());
            packet[2 + user.len()] = password.len() as u8;
            packet[3 + user.len()..packet_size].copy_from_slice(password.as_bytes());

            write(sock, &packet[..packet_size])?;

            let mut buf = [0; 2];
            read_timeout(sock, &mut buf, config.tcp_read_timeout)?;

            if buf[0] != 1 {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidData, "invalid response version").into(),
                );
            }
            if buf[1] != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "password authentication failed",
                )
                .into());
            }
        }
        Ok(())
    }

    fn connect(sock: RawFd, target: &ProxyConf, auth: Option<&Auth>) -> Result<(), Self::E> {
        let config = &*CONFIG;

        let methods = match target.auth {
            Some(_) => 2,
            None => 1,
        };

        let packet = [
            5,                   // version
            methods,             // methods
            Self::auth_id(auth), // method
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

        Self::authenticate(sock, auth)?;

        let mut packet = [0; 264];
        packet[0] = 5; // protocol version
        packet[1] = 1; // connect
        packet[2] = 0; // reserved

        let hnret = find_ip_hostname(target.ip);

        match hnret {
            Some(hn) => {
                // write address
                let len = write_hostname(&mut packet[3..], target, hn)?;
                write(sock, &packet[..len + 3])?;
            }
            None => {
                // write address
                let len = write_addr(&mut packet[3..], target)?;
                write(sock, &packet[..len + 3])?;
            }
        }

        // read response + address on success
        read_response(sock)?;

        Ok(())
    }
}
