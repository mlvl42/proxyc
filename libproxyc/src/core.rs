use crate::error::Error;
use crate::proxy::{self, Proxy};
use crate::util::poll_retry;
use cstr::cstr;
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::libc::{
    self, addrinfo, c_char, c_int, c_void, hostent, servent, size_t, sockaddr, sockaddr_in,
    sockaddr_in6, sockaddr_storage, socklen_t,
};
use nix::poll::{PollFd, PollFlags};
use nix::sys::socket::sockopt::SocketError;
use nix::sys::socket::{getsockopt, AddressFamily, InetAddr, IpAddr, SockAddr};
use nix::unistd::{close, dup2};
use once_cell::sync::Lazy;
use proxyc_common::{ChainType, ProxyConf, ProxyType, ProxycConfig};
use std::mem;
use std::mem::MaybeUninit;
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
    fn inet_aton(cp: *const c_char, inp: *const libc::in_addr) -> c_int;
    fn inet_pton(af: c_int, src: *const c_char, dst: *const c_void) -> c_int;
    fn getservbyname_r(
        name: *const c_char,
        proto: *const c_char,
        result_buf: *mut servent,
        buf: *mut c_char,
        buflen: size_t,
        result: *mut *mut servent,
    ) -> c_int;
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

#[repr(C)]
struct AddrinfoData {
    ai_buf: addrinfo,
    sa_buf: sockaddr_storage,
    addr_name: [c_char; 256],
}

fn contains_numeric_ip(node: *const c_char, sa_buf: *mut sockaddr_storage) -> bool {
    unsafe {
        (*(sa_buf as *mut _ as *mut sockaddr_in)).sin_family = libc::AF_INET as u16;
        let ret = inet_aton(node, &(*(sa_buf as *mut _ as *mut sockaddr_in)).sin_addr);
        if ret != 0 {
            return true;
        }

        (*(sa_buf as *mut _ as *mut sockaddr_in6)).sin6_family = libc::AF_INET6 as u16;
        let ret = inet_pton(
            libc::AF_INET6,
            node,
            &(*(sa_buf as *mut _ as *mut sockaddr_in6)).sin6_addr as *const _ as *const c_void,
        );
        if ret != 0 {
            return true;
        }

        false
    }
}

#[repr(C)]
struct GetHostByNameData {
    hs: hostent,
    raddr: libc::in_addr_t,
    raddr_p: [*const c_char; 2],
    addr_name: [c_char; 256],
}

fn proxyc_gethostbyname(
    name: *const c_char,
    gh: *mut GetHostByNameData,
) -> Result<*mut hostent, std::io::Error> {
    let mut ptr = unsafe { &mut *gh };
    ptr.raddr_p[0] = &ptr.raddr as *const _ as *const c_char;
    ptr.raddr_p[1] = std::ptr::null();

    ptr.hs.h_addr_list = ptr.raddr_p.as_mut_ptr() as *mut *mut i8;
    ptr.hs.h_aliases = ptr.raddr_p[1] as *mut *mut i8;

    // example: assign localhost
    // ptr.raddr = (0x7f000001 as u32).to_be();
    ptr.raddr = 0;
    ptr.hs.h_addrtype = libc::AF_INET;
    ptr.hs.h_length = std::mem::size_of::<libc::in_addr_t>() as i32;

    // TODO: check is numeric ipv4 in name
    // TODO: check is current hostname
    // TODO: check /etc/hosts
    // TODO: assign ip for name

    Ok(&mut ptr.hs)
}

const LOCALHOST_B: [u8; 4] = [127, 0, 0, 1];
pub fn proxyc_getaddrinfo(
    node: *const c_char,
    service: *const c_char,
    hints: *const addrinfo,
    res: *mut *mut addrinfo,
) -> c_int {
    let mut af = libc::AF_INET;
    let ai_data: *mut AddrinfoData =
        unsafe { mem::transmute(libc::calloc(1, mem::size_of::<AddrinfoData>() as size_t)) };
    if ai_data.is_null() {
        return -1;
    }

    let ai_buf = unsafe { &mut (*ai_data).ai_buf as *mut addrinfo };
    let sa_buf = unsafe { &mut (*ai_data).sa_buf as *mut sockaddr_storage };

    unsafe {
        if !node.is_null() && !contains_numeric_ip(node, sa_buf) {
            // fail in case inet_aton / inet_pton did not work and AI_NUMERICHOST
            // // has been set by the caller.
            if !hints.is_null() && (*hints).ai_flags & libc::AI_NUMERICHOST != 0 {
                libc::free(ai_data as *mut _);
                return libc::EAI_NONAME;
            }

            let mut gh: MaybeUninit<GetHostByNameData> = MaybeUninit::uninit();
            let hs = proxyc_gethostbyname(node, gh.as_mut_ptr()).unwrap();
            if !hs.is_null() {
                let p = *hs;
                libc::memcpy(
                    &mut (*(sa_buf as *mut _ as *mut sockaddr_in)).sin_addr as *mut _
                        as *mut c_void,
                    *p.h_addr_list as *const c_void,
                    4,
                );
            }
        } else if !node.is_null() {
            af = (*(sa_buf as *mut _ as *mut sockaddr_in)).sin_family as i32;
        } else if node.is_null() && (*hints).ai_flags & libc::AI_PASSIVE != 0 {
            af = libc::AF_INET;
            libc::memcpy(
                &mut (*(sa_buf as *mut _ as *mut sockaddr_in)).sin_addr as *mut _ as *mut c_void,
                LOCALHOST_B.as_ptr() as *const c_void,
                4,
            );
        }
    }

    let port: u16 = unsafe {
        let mut se: *mut servent = std::ptr::null_mut();
        let mut buf: [u8; 1024] = [0; 1024];
        let mut se_buf: MaybeUninit<servent> = MaybeUninit::uninit();

        getservbyname_r(
            service,
            std::ptr::null(),
            se_buf.as_mut_ptr(),
            buf.as_mut_ptr() as *mut c_char,
            std::mem::size_of_val(&buf),
            &mut se,
        );

        se_buf.assume_init();
        match se.is_null() {
            false => (*se).s_port as u16,
            true => {
                if !service.is_null() {
                    let tmp = libc::atoi(service) as u16;
                    tmp.to_be()
                } else {
                    0
                }
            }
        }
    };

    unsafe {
        match af {
            libc::AF_INET => {
                (*(sa_buf as *mut _ as *mut sockaddr_in)).sin_port = port;
            }
            _ => {
                (*(sa_buf as *mut _ as *mut sockaddr_in6)).sin6_port = port;
            }
        };
    }

    unsafe {
        (*ai_buf).ai_addr = sa_buf as *mut sockaddr;

        (*ai_buf).ai_next = std::ptr::null_mut() as *mut addrinfo;
        (*sa_buf).ss_family = af as u16;
        (*ai_buf).ai_family = af;
        match af {
            libc::AF_INET => {
                (*ai_buf).ai_addrlen = std::mem::size_of::<sockaddr_in>() as u32;
            }
            _ => (*ai_buf).ai_addrlen = std::mem::size_of::<sockaddr_in6>() as u32,
        };

        if !hints.is_null() {
            (*ai_buf).ai_socktype = (*hints).ai_socktype;
            (*ai_buf).ai_flags = (*hints).ai_flags;
            (*ai_buf).ai_protocol = (*hints).ai_protocol;
        } else {
            (*ai_buf).ai_flags = libc::AI_V4MAPPED | libc::AI_ADDRCONFIG;
        }
    }

    unsafe {
        *res = ai_buf;
    }

    0
}
