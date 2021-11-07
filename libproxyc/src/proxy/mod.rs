pub use http::Http;
use proxyc_common::{Auth, ProxyConf};
pub use socks::{Socks4, Socks5};
use std::os::unix::io::RawFd;

mod http;
mod socks;

pub trait Proxy {
    type E;
    fn connect(sock: RawFd, target: &ProxyConf, auth: Option<&Auth>) -> Result<(), Self::E>;
    fn authenticate(_sock: RawFd, _auth: Option<&Auth>) -> Result<(), Self::E> {
        Ok(())
    }
}
