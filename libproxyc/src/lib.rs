extern crate log;
extern crate pretty_env_logger;

use nix::libc::{c_int, sockaddr, socklen_t};
use nix::sys::socket::{
    getsockopt, socket, sockopt, AddressFamily, InetAddr, IpAddr, SockAddr, SockFlag, SockType,
};
use nix::unistd::{close, dup2};
use proxyc::core;
use std::io::Write;
use std::os::unix::io::RawFd;

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

// TODO should perhaps become a step of the chain
// TODO handle conf and args
// TODO handle socks
// TODO handle chain
fn connect_proxyc(sock: RawFd, _target: &SockAddr) -> Result<(), Box<dyn std::error::Error>> {
    // create new socket with similar settings
    // let new_sock = socket(target.family(), SockType::Stream, SockFlag::empty(), None)?;

    //FIXME only for testing
    let target = SockAddr::new_inet(InetAddr::new(IpAddr::new_v4(127, 0, 0, 1), 8080));
    let new_sock = socket(target.family(), SockType::Stream, SockFlag::empty(), None)?;

    // (timed) connect to dest
    let mut stream = core::connect(new_sock, &target)?;

    // TODO:
    // based on the current type strict, dynamic, random etc..
    // (calc_alive ?)
    // - 1 select proxy from list
    // - 2 start chain
    // - 3 select another proxy from list
    // - 4 tunnel previous to this one
    // - 5 repeat 3
    // - 6 connect to target

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
    let c_connect = CONNECT.expect("Cannot load symbol 'connect'");
    let addr_opt = unsafe { from_libc_sockaddr(address) };

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
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug");
    }

    pretty_env_logger::init();
}
