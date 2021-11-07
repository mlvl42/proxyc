use crate::error::Error;
use crate::proxy::{self, Proxy};
use crate::util::poll_retry;
use cstr::cstr;
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::libc::{self, addrinfo, c_char, c_int, sockaddr, socklen_t};
use nix::poll::{PollFd, PollFlags};
use nix::sys::socket::sockopt::SocketError;
use nix::sys::socket::{getsockopt, AddressFamily, InetAddr, IpAddr, SockAddr};
use nix::unistd::{close, dup2};
use once_cell::sync::Lazy;
use proxyc_common::{ChainType, ProxyConf, ProxyType, ProxycConfig};
use std::os::unix::io::RawFd;

type ConnectFn =
    unsafe extern "C" fn(socket: RawFd, address: *const sockaddr, len: socklen_t) -> c_int;

type GetAddrInfoFn = unsafe extern "C" fn(
    node: *const c_char,
    service: *const c_char,
    hints: *const addrinfo,
    res: *mut *mut addrinfo,
) -> c_int;

pub static CONNECT: Lazy<Option<ConnectFn>> = Lazy::new(|| unsafe {
    std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, cstr!("connect").as_ptr()))
});

pub static GETADDRINFO: Lazy<Option<GetAddrInfoFn>> = Lazy::new(|| unsafe {
    std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, cstr!("getaddrinfo").as_ptr()))
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

fn chain_step(sock: RawFd, from: &ProxyConf, to: &ProxyConf) -> Result<(), Error> {
    debug!("chain {} <=> {}", from, to);

    println!("{:?}", to);
    match from.proto {
        ProxyType::Raw => Ok(()),
        ProxyType::Http => Ok(proxy::Http::connect(sock, to, from.auth.as_ref())?),
        ProxyType::Socks4 => Ok(proxy::Socks4::connect(sock, to, from.auth.as_ref())?),
        ProxyType::Socks5 => Ok(proxy::Socks5::connect(sock, to, from.auth.as_ref())?),
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
        auth: None,
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
