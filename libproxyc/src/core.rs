use crate::error::Error;
use cstr::cstr;
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::libc;
use nix::libc::{c_int, sockaddr, socklen_t};
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::socket::sockopt::SocketError;
use nix::sys::socket::{getsockopt, AddressFamily, InetAddr, SockAddr};
use nix::unistd::read;
use once_cell::sync::Lazy;
use std::convert::TryInto;
use std::os::unix::io::RawFd;
use std::time::Instant;

type ConnectFn =
    unsafe extern "C" fn(socket: RawFd, address: *const sockaddr, len: socklen_t) -> c_int;

pub static CONNECT: Lazy<Option<ConnectFn>> = Lazy::new(|| unsafe {
    std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, cstr!("connect").as_ptr()))
});

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
                    _ => return Err(Error::SocketError),
                };
            }
            _ => return Err(Error::ConnectError("poll_retry".into())),
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
