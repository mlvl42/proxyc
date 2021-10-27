use crate::error::Error;
use byteorder::{BigEndian, WriteBytesExt};
use cstr::cstr;
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::libc;
use nix::libc::{c_int, sockaddr, socklen_t};
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::socket::sockopt::SocketError;
use nix::sys::socket::{getsockopt, AddressFamily, InetAddr, IpAddr, SockAddr};
use nix::unistd::read;
use nix::unistd::{close, dup2, write};
use once_cell::sync::Lazy;
use proxyc_common::{ChainType, ProxyConf, ProxyType, ProxycConfig};
use std::convert::TryInto;
use std::io;
use std::io::Write;
use std::os::unix::io::RawFd;
use std::time::Instant;

type ConnectFn =
    unsafe extern "C" fn(socket: RawFd, address: *const sockaddr, len: socklen_t) -> c_int;

pub static CONNECT: Lazy<Option<ConnectFn>> = Lazy::new(|| unsafe {
    std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, cstr!("connect").as_ptr()))
});

pub static CONFIG: Lazy<ProxycConfig> =
    Lazy::new(|| ProxycConfig::from_env().expect("failed to parse config"));

/// Initiate a connection on a socket
///
/// We can't use nix::sys::socket::connect since it would call our hooked
/// connect function and recurse infinitely.
// pub fn connect(fd: RawFd, addr: &SockAddr) -> Result<(), Error> {
//     let c_connect = CONNECT.expect("Cannot load symbol 'connect'");

//     let res = unsafe {
//         let (ptr, len) = addr.as_ffi_pair();
//         c_connect(fd, ptr, len)
//     };

//     if let Err(x) = Errno::result(res).map(drop).map_err(|x| x.into()) {
//         Err(x)
//     } else {
//         // Ok(unsafe { TcpStream::from_raw_fd(fd) })
//         Ok(())
//     }
// }

/// Initiate a connection on a socket, timeout after specified time in
/// milliseconds.
pub fn timed_connect(fd: RawFd, addr: &SockAddr, timeout: usize) -> Result<(), Error> {
    let c_connect = CONNECT.expect("Cannot load symbol 'connect'");

    let mut fds = [PollFd::new(fd, PollFlags::POLLOUT)];
    let mut oflag = OFlag::empty();

    oflag.toggle(OFlag::O_NONBLOCK);
    match fcntl(fd, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)) {
        Ok(_) => (),
        Err(e) => error!("fcntl NONBLOCK error: {}", e),
    };

    let res = unsafe {
        let (ptr, len) = addr.as_ffi_pair();
        c_connect(fd, ptr, len)
    };

    if let (-1, Errno::EINPROGRESS) = (res, errno()) {
        let ret = poll_retry(&mut fds, timeout)?;

        match ret {
            1 => {
                match getsockopt(fd, SocketError)? {
                    0 => (),
                    _ => return Err(Error::Socket),
                };
            }
            _ => return Err(Error::Connect("poll_retry".into())),
        };
    }

    oflag.toggle(OFlag::O_NONBLOCK);
    match fcntl(fd, FcntlArg::F_SETFL(oflag)) {
        Ok(_) => (),
        Err(e) => error!("fcntl BLOCK error: {}", e),
    };

    match Errno::result(res) {
        Ok(_) => Ok(()),
        Err(Errno::EINPROGRESS) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn read_timeout(fd: RawFd, mut buf: &mut [u8], timeout: usize) -> Result<(), Error> {
    let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];

    while !buf.is_empty() {
        poll_retry(&mut fds, timeout)?;

        if fds[0]
            .revents()
            .map_or(true, |e| !e.contains(PollFlags::POLLIN))
        {
            return Err(Error::Generic("POLLING poll flag missing".into()));
        }

        match read(fd, buf) {
            Ok(0) => break,
            Ok(n) => {
                let tmp = buf;
                buf = &mut tmp[n..];
            }
            Err(e) => return Err(e.into()),
        }
    }
    if !buf.is_empty() {
        Err(Error::MissingData)
    } else {
        Ok(())
    }
}

pub fn poll_retry(mut fds: &mut [PollFd], timeout: usize) -> Result<i32, Error> {
    let now = Instant::now();
    let mut remaining: i32 = timeout.try_into().unwrap();
    loop {
        let ret = poll(&mut fds, remaining);
        let elapsed = now.elapsed().as_millis();
        remaining = remaining
            .checked_sub(elapsed.try_into().unwrap())
            .unwrap_or(0);

        if remaining == 0 {
            return Err(Error::Timeout);
        }

        match ret {
            Ok(nfds) => return Ok(nfds),
            Err(Errno::EINTR) => (),
            Err(e) => return Err(e.into()),
        }
    }
}

/// Creates a `SockAddr` struct from libc's sockaddr.
///
/// Supports only the following address families: Inet (v4 & v6)
/// Returns None for unsupported families.
///
/// # Safety
///
/// unsafe because it takes a raw pointer as argument.
pub unsafe fn from_libc_sockaddr(addr: *const libc::sockaddr) -> Option<SockAddr> {
    if addr.is_null() {
        None
    } else {
        match AddressFamily::from_i32(i32::from((*addr).sa_family)) {
            Some(AddressFamily::Inet) => Some(SockAddr::Inet(InetAddr::V4(
                *(addr as *const libc::sockaddr_in),
            ))),
            Some(AddressFamily::Inet6) => Some(SockAddr::Inet(InetAddr::V6(
                *(addr as *const libc::sockaddr_in6),
            ))),
            Some(_) | None => None,
        }
    }
}

pub fn errno() -> Errno {
    unsafe { Errno::from_i32(*__errno_location()) }
}

pub fn set_errno(errno: Errno) {
    unsafe {
        *__errno_location() = errno as i32;
    }
}

extern "C" {
    pub fn __errno_location() -> *mut i32;
}

/// main logic

fn chain_start(sock: RawFd, proxy: &ProxyConf) -> Result<(), Error> {
    let config = &*CONFIG;

    debug!("start chain {}", proxy);
    let target = SockAddr::new_inet(InetAddr::new(IpAddr::from_std(&proxy.ip), proxy.port));
    timed_connect(sock, &target, config.tcp_connect_timeout)?;
    Ok(())
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

fn chain_step(sock: RawFd, from: &ProxyConf, to: &ProxyConf) -> Result<(), Error> {
    debug!("chain {} <=> {}", from, to);
    let config = &*CONFIG;

    match from.proto {
        ProxyType::Raw => Ok(()),
        ProxyType::Http => {
            let ip = match to.ip {
                std::net::IpAddr::V4(addr) => addr.to_string(),
                std::net::IpAddr::V6(addr) => addr.to_string(),
            };

            let packet = format!("CONNECT {}:{} HTTP/1.0\r\n\r\n", ip, to.port);
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
        ProxyType::Socks4 => {
            let mut packet = vec![];

            let _ = packet.write_u8(4); // version
            let _ = packet.write_u8(1); // connect

            // TODO handle auth and proxy dns

            match to.ip {
                std::net::IpAddr::V4(addr) => {
                    packet.write_u16::<BigEndian>(to.port)?;
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
        ProxyType::Socks5 => {
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
                return Err(
                    io::Error::new(io::ErrorKind::Other, "no acceptable auth method").into(),
                );
            }

            let mut packet = [0; 264];
            packet[0] = 5; // protocol version
            packet[1] = 1; // connect
            packet[2] = 0; // reserved

            // write address
            let len = write_addr(&mut packet[3..], to)?;
            write(sock, &packet[..len + 3])?;

            // read response + address on success
            read_response(sock)?;

            Ok(())
        }
    }
}

// TODO handle ipv6
pub fn connect_proxyc(sock: RawFd, ns: RawFd, target: &SockAddr) -> Result<(), Error> {
    let config = &*CONFIG;

    // Build a proxyconf from the target sockaddr
    let (target_ip, target_port) = match target {
        SockAddr::Inet(x) => {
            let tmp = x.to_std();
            Ok((tmp.ip(), tmp.port()))
        }
        _ => Err(Error::Generic("not an inet sockaddr".into())),
    }?;

    let target_conf = ProxyConf {
        proto: ProxyType::Raw,
        ip: target_ip,
        port: target_port,
    };

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
            // start the chain by connecting to the first proxy
            chain_start(ns, config.proxies.first().unwrap())?;

            // chain each proxy ends
            for w in config.proxies.windows(2) {
                chain_step(ns, &w[0], &w[1])?;
            }
            // chain the target
            chain_step(ns, config.proxies.last().unwrap(), &target_conf)?;

            Ok(ns)
        }
        _ => Err(Error::Generic("chain type not handled".into())),
    }?;

    dup2(new_sock, sock)?;
    close(new_sock)?;

    debug!("connected to {}", target.to_str());
    Ok(())
}
