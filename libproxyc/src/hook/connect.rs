use crate::core;
use crate::error::Error;
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::libc::{c_int, sockaddr, socklen_t};
use nix::sys::socket::{getsockopt, socket, sockopt, AddressFamily, SockAddr, SockFlag, SockType};
use nix::unistd::close;
use std::os::unix::io::RawFd;

fn check_socket(sock: RawFd, addr: &SockAddr) -> Result<(), Error> {
    let socktype = getsockopt(sock, sockopt::SockType).unwrap();
    let fam = addr.family();

    if !((fam == (AddressFamily::Inet) || fam == AddressFamily::Inet6)
        && socktype == SockType::Stream)
    {
        // socket is not of the appropriate type
        return Err(Error::Socket);
    }

    let config = &*core::CONFIG;
    if config.ignore_subnets.is_empty() {
        return Ok(());
    }

    // check if the target should be ignored
    let (target_ip, target_port) = match addr {
        SockAddr::Inet(x) => {
            let tmp = x.to_std();
            Ok((tmp.ip(), tmp.port()))
        }
        _ => Err(Error::Socket),
    }?;

    for i in config.ignore_subnets.iter() {
        if let Some(p) = i.port {
            if p == target_port {
                return Err(Error::Socket);
            }
        }

        if let std::net::IpAddr::V4(ip) = target_ip {
            if i.cidr.contains(&ip) {
                return Err(Error::Socket);
            }
        }
    }

    Ok(())
}

#[no_mangle]
pub fn connect(sock: RawFd, address: *const sockaddr, len: socklen_t) -> c_int {
    let c_connect = core::CONNECT.expect("Cannot load symbol 'connect'");
    let addr_opt = unsafe { core::from_libc_sockaddr(address) };

    trace!("connect hooked");

    if let Some(addr) = addr_opt {
        // if the socket is not of the correct type, or the target address
        // should be ignored, use the true connect call.
        if check_socket(sock, &addr).is_ok() {
            let ns = match socket(addr.family(), SockType::Stream, SockFlag::empty(), None) {
                Ok(s) => s,
                Err(_e) => return -1,
            };

            // store original flags set by caller.
            // we will mess with it later and thus need to reset it before
            // returning.
            let mut flags = match fcntl(sock, FcntlArg::F_GETFL) {
                Ok(f) => OFlag::from_bits_truncate(f),
                Err(_) => return -1,
            };
            let flags_orig = flags;

            if flags.contains(OFlag::O_NONBLOCK) {
                flags.toggle(OFlag::O_NONBLOCK);
                fcntl(sock, FcntlArg::F_SETFL(flags)).expect("fcntl force blocking failed");
            }

            match core::connect_proxyc(sock, ns, &addr) {
                Ok(_) => match fcntl(sock, FcntlArg::F_SETFL(flags_orig)) {
                    Ok(_) => {
                        return 0;
                    }
                    Err(e) => {
                        error!("fcntl apply original flags error: {}", e)
                    }
                },
                Err(e) => {
                    close(ns).ok();
                    error!("{}", e);
                    core::set_errno(Errno::ECONNREFUSED); // for nmap
                    return -1;
                }
            }
        }
    }

    unsafe { c_connect(sock, address, len) }
}
