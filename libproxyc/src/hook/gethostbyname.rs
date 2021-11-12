use crate::core;
use nix::libc::{c_char, hostent};
use std::mem::MaybeUninit;

static mut GETHOSTBYNAME_DATA: MaybeUninit<core::GetHostByNameData> = MaybeUninit::uninit();

#[no_mangle]
fn gethostbyname(name: *const c_char) -> *mut hostent {
    let c_gethostbyname = core::GETHOSTBYNAME.expect("Cannot load symbol 'gethostbyname'");

    trace!("gethostbyname hooked");

    let config = &*core::CONFIG;
    if config.proxy_dns {
        let ptr = unsafe { GETHOSTBYNAME_DATA.as_mut_ptr() };
        match core::proxyc_gethostbyname(name, ptr) {
            Ok(hs) => hs,
            Err(e) => {
                error!("{}", e);
                std::ptr::null_mut()
            }
        }
    } else {
        unsafe { c_gethostbyname(name) }
    }
}
