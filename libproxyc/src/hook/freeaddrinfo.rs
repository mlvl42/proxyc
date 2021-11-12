use crate::core;
use nix::libc::{self, addrinfo, c_void};

#[no_mangle]
fn freeaddrinfo(res: *mut addrinfo) {
    let c_freeaddrinfo = core::FREEADDRINFO.expect("Cannot load symbol 'freeaddrinfo'");
    let config = &*core::CONFIG;

    trace!("freeaddrinfo hooked");

    if config.proxy_dns {
        if !res.is_null() {
            unsafe { libc::free(res as *mut c_void) };
        }
    } else {
        unsafe { c_freeaddrinfo(res) };
    }
}
