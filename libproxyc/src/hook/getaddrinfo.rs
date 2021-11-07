use crate::core;
use nix::libc::{addrinfo, c_char, c_int};

#[no_mangle]
fn getaddrinfo(
    node: *const c_char,
    service: *const c_char,
    hints: *const addrinfo,
    res: *mut *mut addrinfo,
) -> c_int {
    let c_getaddrinfo = core::GETADDRINFO.expect("Cannot load symbol 'getaddrinfo'");

    info!("getaddrinfo hooked");

    let config = &*core::CONFIG;
    if config.proxy_dns {
        core::proxyc_getaddrinfo(node, service, hints, res)
    } else {
        unsafe { c_getaddrinfo(node, service, hints, res) }
    }
}
