use log::LevelFilter;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyConf {
    #[serde(rename = "type")]
    pub proto: ProxyType,
    pub ip: std::net::IpAddr,
    pub port: u16,
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
