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
        return Err(Error::Generic("bad socket, very bad".into()));
    }

    Ok(())
}

#[no_mangle]
pub fn connect(sock: RawFd, address: *const sockaddr, len: socklen_t) -> c_int {
    let c_connect = core::CONNECT.expect("Cannot load symbol 'connect'");
    let addr_opt = unsafe { core::from_libc_sockaddr(address) };

    if let Some(addr) = addr_opt {
        if check_socket(sock, &addr).is_ok() {
            let ns = match socket(addr.family(), SockType::Stream, SockFlag::empty(), None) {
                Ok(s) => s,
                Err(_e) => return -1,
            };

            // store original flags set by caller.
            // we will mess with it later and thus need to reset it before
            // returning.
            let flags = match fcntl(sock, FcntlArg::F_GETFL) {
                Ok(f) => OFlag::from_bits_truncate(f),
                Err(_) => return -1,
            };
            let flags_orig = flags;

            //if flags.contains(OFlag::O_NONBLOCK) {
            //    flags.toggle(OFlag::O_NONBLOCK);
            //    fcntl(sock, FcntlArg::F_SETFL(flags)).expect("fcntl force blocking failed");
            //}

            match core::connect_proxyc(sock, ns, &addr) {
                Ok(_) => {
                    info!("connect success");
                    match fcntl(sock, FcntlArg::F_SETFL(flags_orig)) {
                        Ok(_) => {
                            return 0;
                        }
                        Err(e) => {
                            error!("fcntl apply original flags error: {}", e)
                        }
                    }
                }
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
