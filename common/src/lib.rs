use log::LevelFilter;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;
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
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let proto = match url.scheme() {
            "socks4" => ProxyType::Socks4,
            "socks5" => ProxyType::Socks5,
            "http" => ProxyType::Http,
            "raw" => ProxyType::Raw,
            _ => {
                return Err(ConfigError::ParseError(format!(
                    "scheme {:?} not handled",
                    url.scheme()
                )))
            }
        };

        let ip = url
            .host()
            .ok_or(ConfigError::ParseError("missing host".into()))?;
        let ip = std::net::IpAddr::from_str(&ip.to_string()).map_err(|_| {
            ConfigError::ParseError(format!("invalid ip address {:?}", &ip.to_string()))
        })?;
        let port = url
            .port()
            .ok_or(ConfigError::ParseError("missing port".into()))?;

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

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("parse error: {0}")]
    ParseError(String),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("toml error")]
    TomlError(#[from] toml::de::Error),
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),
    #[error("missing environment variable: {0}")]
    MissingEnv(String),
}

fn default_tcp_read() -> usize {
    15000
}

fn default_tcp_connect() -> usize {
    8000
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxycConfig {
    #[serde(rename = "proxy")]
    pub proxies: Vec<ProxyConf>,
    pub chain_type: ChainType,
    #[serde(with = "LevelFilterRef")]
    pub log_level: LevelFilter,
    #[serde(default = "default_tcp_read")]
    pub tcp_read_timeout: usize,
    #[serde(default = "default_tcp_connect")]
    pub tcp_connect_timeout: usize,
}

impl ProxycConfig {
    pub fn new(path: &PathBuf) -> Result<Self, ConfigError> {
        let mut file = std::fs::File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config: ProxycConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let content = std::env::var("PROXYC_CONFIG")
            .map_err(|_| ConfigError::MissingEnv("PROXYC_CONFIG".into()))?;
        let config: ProxycConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn to_json(&self) -> Result<String, ConfigError> {
        Ok(serde_json::to_string(self)?)
    }
}
