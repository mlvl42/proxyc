use log::LevelFilter;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

#[derive(Debug, Serialize, Deserialize)]
#[serde(remote = "LevelFilter")]
#[serde(rename_all = "lowercase")]
enum LevelFilterRef {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyType {
    Raw,
    Http,
    Socks4,
    Socks5,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChainType {
    Strict,
    Dynamic,
    Random,
}

impl FromStr for ChainType {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "strict" => ChainType::Strict,
            "dynamic" => ChainType::Dynamic,
            "random" => ChainType::Random,
            _ => return Err(io::Error::new(io::ErrorKind::Other, "invalid chain type")),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyConf {
    #[serde(rename = "type")]
    pub proto: ProxyType,
    pub ip: std::net::IpAddr,
    pub port: u16,
}

impl FromStr for ProxyConf {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let proto = match url.scheme() {
            "socks4" => ProxyType::Socks4,
            "socks5" => ProxyType::Socks5,
            "http" => ProxyType::Http,
            "raw" => ProxyType::Raw,
            _ => return Err(io::Error::new(io::ErrorKind::Other, "scheme not handled")),
        };

        let ip = url
            .host()
            .ok_or("missing host")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let ip = std::net::IpAddr::from_str(&ip.to_string())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let port = url
            .port()
            .ok_or("missing port")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(ProxyConf { proto, ip, port })
    }
}

impl fmt::Display for ProxyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let proto = match *self {
            ProxyType::Raw => "raw",
            ProxyType::Http => "http",
            ProxyType::Socks4 => "socks4",
            ProxyType::Socks5 => "socks5",
        };
        write!(f, "{}", proto)
    }
}

impl fmt::Display for ProxyConf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}:{}", self.proto, self.ip, self.port)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxycConfig {
    #[serde(rename = "proxy")]
    pub proxies: Vec<ProxyConf>,
    pub chain_type: ChainType,
    #[serde(with = "LevelFilterRef")]
    pub log_level: LevelFilter,
    pub tcp_read_timeout: Option<usize>,
    pub tcp_connect_timeout: Option<usize>,
}

impl ProxycConfig {
    pub fn new(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = std::fs::File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config: ProxycConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::env::var("PROXYC_CONFIG")?;
        let config: ProxycConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
