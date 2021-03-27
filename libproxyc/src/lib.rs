#[macro_use]
extern crate log;
extern crate pretty_env_logger;

mod core;

use byteorder::{BigEndian, WriteBytesExt};
use nix::libc::{c_int, sockaddr, socklen_t};
use nix::sys::socket::{
    getsockopt, recv, send, socket, sockopt, AddressFamily, InetAddr, IpAddr, MsgFlags, SockAddr,
    SockFlag, SockType,
};
use nix::unistd::{close, dup2};
use once_cell::sync::Lazy;
use proxyc_common::{ChainType, ProxyConf, ProxyType, ProxycConfig};
use std::io;
use std::io::Write;
use std::os::unix::io::RawFd;

static CONFIG: Lazy<ProxycConfig> = Lazy::new(|| ProxycConfig::from_env().unwrap());

/// Converts a value from host byte order to network byte order.
pub fn htons(u: u16) -> u16 {
    u.to_be()
}
/// Converts a value from network byte order to host byte order.
pub fn ntohs(u: u16) -> u16 {
    u16::from_be(u)
}

/// Converts a value from host byte order to network byte order.
#[inline]
pub fn htonl(u: u32) -> u32 {
    u.to_be()
}

/// Converts a value from network byte order to host byte order.
#[inline]
pub fn ntohl(u: u32) -> u32 {
    u32::from_be(u)
}

fn chain_start(sock: RawFd, proxy: &ProxyConf) -> Result<(), Box<dyn std::error::Error>> {
    let target = SockAddr::new_inet(InetAddr::new(IpAddr::from_std(&proxy.ip), proxy.port));
    core::connect(sock, &target)?;
    Ok(())
}

fn recv_exact(sock: RawFd, mut buf: &mut [u8]) -> Result<(), Box<dyn std::error::Error>> {
    while !buf.is_empty() {
        match recv(sock, buf, MsgFlags::empty()) {
            Ok(0) => break,
            Ok(n) => {
                let tmp = buf;
                buf = &mut tmp[n..];
            }
            Err(e) => return Err(e.into()),
        }
    }
    if !buf.is_empty() {
        Err("failed to fill whole buffer".into())
    } else {
        Ok(())
    }
}

fn read_response(sock: RawFd) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = [0; 4];
    recv_exact(sock, &mut buf)?;

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
    recv_exact(sock, &mut buf)?;

    Ok(())
}

fn write_addr(
    mut packet: &mut [u8],
    target: &ProxyConf,
) -> Result<usize, Box<dyn std::error::Error>> {
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
        } // FIXME
          // TargetAddr::Domain(ref domain, port) => {
          //     packet.write_u8(3).unwrap();
          //     if domain.len() > u8::max_value() as usize {
          //         return Err(io::Error::new(
          //             io::ErrorKind::InvalidInput,
          //             "domain name too long",
          //         ));
          //     }
          //     packet.write_u8(domain.len() as u8).unwrap();
          //     packet.write_all(domain.as_bytes()).unwrap();
          //     packet.write_u16::<BigEndian>(port).unwrap();
          // }
    }
    Ok(start_len - packet.len())
}

fn chain_step(
    sock: RawFd,
    from: &ProxyConf,
    to: &ProxyConf,
) -> Result<(), Box<dyn std::error::Error>> {
    let flags = MsgFlags::empty();

    match from.proto {
        ProxyType::Raw => Ok(()),
        ProxyType::Socks5 => {
            // TODO handle auth

            let packet = [
                5, // version
                1, // methods
                0, // no auth
            ];
            send(sock, &packet, flags)?;

            let mut buf = [0; 2];
            recv_exact(sock, &mut buf)?;

            let response_version = buf[0];
            let selected_method = buf[1];

            if response_version != 5 {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidData, "invalid response version").into(),
                );
            }

            if selected_method == 0xff {
                return Err(
                    io::Error::new(io::ErrorKind::Other, "no acceptable auth method").into(),
                );
            }

            let mut packet = [0; 264];
            // FIXME still needed ?
            packet[0] = 5; // protocol version
            packet[1] = 1; // connect
            packet[2] = 0; // reserved

            // write address
            let len = write_addr(&mut packet[3..], &to)?;
            send(sock, &packet[..len + 3], flags)?;

            // read response + address on success
            read_response(sock)?;

            Ok(())
        }
        _ => Err("protocol not handled".into()),
    }
}

// TODO handle socks
// TODO handle ipv6
fn connect_proxyc(sock: RawFd, target: &SockAddr) -> Result<(), Box<dyn std::error::Error>> {
    let config = &*CONFIG;

    // Build a proxyconf from the target sockaddr
    let (target_ip, target_port) = match target {
        SockAddr::Inet(x) => {
            let tmp = x.to_std();
            Ok((tmp.ip(), tmp.port()))
        }
        _ => Err("not an inet sockaddr"),
    }?;

    let target_conf = ProxyConf {
        proto: ProxyType::Raw,
        ip: target_ip,
        port: target_port,
    };

    // create new socket with similar settings
    // let new_sock = socket(target.family(), SockType::Stream, SockFlag::empty(), None)?;

    //FIXME only for testing
    // let target = SockAddr::new_inet(InetAddr::new(IpAddr::new_v4(127, 0, 0, 1), 1080));
    // let new_sock = socket(target.family(), SockType::Stream, SockFlag::empty(), None)?;

    // (timed) connect to dest
    // let _stream = core::connect(new_sock, &target)?;

    // TODO:
    // based on the current type strict, dynamic, random etc..
    // (calc_alive ?)
    // - 1 select proxy from list
    // - 2 start chain
    // - 3 select another proxy from list
    // - 4 tunnel previous to this one
    // - 5 repeat 3
    // - 6 connect to target
    let new_sock = match config.chain_type {
        ChainType::Strict => {
            let ns = socket(target.family(), SockType::Stream, SockFlag::empty(), None)?;

            // build a list of tuple of proxies to connect to.
            // first is None to to start the chain.
            // last is the target.
            let proxies: Vec<(Option<&ProxyConf>, Option<&ProxyConf>)> = {
                let mut prx = vec![];

                prx.push((None, config.proxies.get(0)));

                if config.proxies.len() > 1 {
                    let iter = config.proxies.windows(2).map(|w| (w.get(0), w.get(1)));
                    prx.extend(iter);
                }

                prx.push((config.proxies.last(), Some(&target_conf)));
                prx
            };

            for (p1, p2) in proxies {
                debug!("current proxies: {:?}, {:?}", p1, p2);
                let to = p2.unwrap();
                match p1 {
                    None => chain_start(ns, to)?,
                    Some(p) => chain_step(ns, p, to)?,
                }
            }

            Ok(ns)
        }
        _ => Err("chain type not handled"),
    }?;

    dup2(new_sock, sock)?;
    close(new_sock)?;

    debug!("connected to {}", target.to_str());
    Ok(())
}

fn check_socket(sock: RawFd, addr: &SockAddr) -> Result<(), Box<dyn std::error::Error>> {
    let socktype = getsockopt(sock, sockopt::SockType).unwrap();
    let fam = addr.family();

    if !((fam == (AddressFamily::Inet) || fam == AddressFamily::Inet6)
        && socktype == SockType::Stream)
    {
        return Err("bad socket, very bad".into());
    }

    Ok(())
}

#[no_mangle]
fn connect(sock: RawFd, address: *const sockaddr, len: socklen_t) -> c_int {
    // let addr_opt = unsafe { address.as_ref() };
    let c_connect = core::CONNECT.expect("Cannot load symbol 'connect'");
    let addr_opt = unsafe { core::from_libc_sockaddr(address) };

    if let Some(addr) = addr_opt {
        if check_socket(sock, &addr).is_ok() {
            match connect_proxyc(sock, &addr) {
                Ok(_) => return 0,
                Err(e) => {
                    error!("{}", e);
                    return -1;
                }
            }
        }
    }

    unsafe { c_connect(sock, address, len) }
}

/// This is called when our dynamic library is loaded, so we setup our internals
/// here.
#[no_mangle]
#[link_section = ".init_array"]
static LD_PRELOAD_INIT: extern "C" fn() = self::init;
extern "C" fn init() {
    let config = &*CONFIG;
    std::env::set_var("RUST_LOG", config.log_level.to_string());

    pretty_env_logger::init();

    debug!("config: {:?}", config);
}
