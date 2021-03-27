use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub enum ProxyType {
    Raw,
    Http,
    Socks4,
    Socks5,
}

#[derive(Debug, Deserialize)]
pub enum ChainType {
    Strict,
    Dynamic,
    Random,
}

#[derive(Debug, Deserialize)]
pub struct ProxyConf {
    #[serde(rename = "type")]
    pub proto: ProxyType,
    pub ip: std::net::IpAddr,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct ProxycConfig {
    #[serde(rename = "proxy")]
    pub proxies: Vec<ProxyConf>,
    pub chain_type: ChainType,
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
}
