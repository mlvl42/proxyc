use cstr::cstr;
use nix::errno::Errno;
use nix::libc;
use nix::libc::{c_int, sockaddr, socklen_t};
use nix::sys::socket::{AddressFamily, InetAddr, SockAddr};
use once_cell::sync::Lazy;
use std::os::unix::io::RawFd;

type ConnectFn =
    unsafe extern "C" fn(socket: RawFd, address: *const sockaddr, len: socklen_t) -> c_int;

pub static CONNECT: Lazy<Option<ConnectFn>> = Lazy::new(|| unsafe {
    std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, cstr!("connect").as_ptr()))
});

/// Initiate a connection on a socket
///
/// We can't use nix::sys::socket::connect since it would call our hooked
/// connect function and recurse infinitely.
pub fn connect(fd: RawFd, addr: &SockAddr) -> Result<(), Box<dyn std::error::Error>> {
    let c_connect = CONNECT.expect("Cannot load symbol 'connect'");

    let res = unsafe {
        let (ptr, len) = addr.as_ffi_pair();
        c_connect(fd, ptr, len)
    };

    if let Err(x) = Errno::result(res).map(drop).map_err(|x| x.into()) {
        Err(x)
    } else {
        // Ok(unsafe { TcpStream::from_raw_fd(fd) })
        Ok(())
    }
}

// TODO: connect timeout, adapt from rust std
// https://github.com/rust-lang/rust/blob/db492ecd5ba6bd82205612cebb9034710653f0c2/library/std/src/sys/unix/net.rs#L124
// pub fn connect_timeout(fd: RawFd, addr: &SockAddr, timeout: Duration) -> Result<TcpStream, Box<dyn std::error::Error>> {
// }

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
