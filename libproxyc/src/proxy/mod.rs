pub use http::Http;
use proxyc_common::ProxyConf;
pub use socks::{Socks4, Socks5};
use std::os::unix::io::RawFd;

mod http;
mod socks;

pub trait Proxy {
    type E;
    fn connect(sock: RawFd, target: &ProxyConf) -> Result<(), Self::E>;
}
