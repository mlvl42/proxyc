use cidr::Ipv4Cidr;
use log::LevelFilter;
use serde::de::{self, DeserializeSeed};
use serde::{Deserialize, Deserializer, Serialize};
use std::default::Default;
use std::fmt;
use std::io;
use std::io::Read;
use std::marker::PhantomData;
use std::ops::Not;
use std::path::Path;
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

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyType {
    Raw,
    Http,
    Socks4,
    Socks5,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Auth {
    UserPassword(String, String),
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
    pub auth: Option<Auth>,
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
            .ok_or_else(|| ConfigError::ParseError("missing host".into()))?;
        let ip = std::net::IpAddr::from_str(&ip.to_string()).map_err(|_| {
            ConfigError::ParseError(format!("invalid ip address {:?}", &ip.to_string()))
        })?;
        let port = url
            .port()
            .ok_or_else(|| ConfigError::ParseError("missing port".into()))?;

        let username = url.username().is_empty().not().then(|| url.username());
        let password = url.password();

        if (username.is_some() || password.is_some())
            && (proto != ProxyType::Socks5 && proto != ProxyType::Http)
        {
            return Err(ConfigError::ParseError(
                "authentication is only implemented for socks5 and http".into(),
            ));
        }

        let auth = match (username, password) {
            (Some(u), Some(p)) => Some(Auth::UserPassword(u.into(), p.into())),
            (None, None) => None,
            _ => {
                return Err(ConfigError::ParseError(
                    "unhandled authentication method".into(),
                ));
            }
        };

        Ok(ProxyConf {
            proto,
            ip,
            port,
            auth,
        })
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
        if let Some(auth) = &self.auth {
            match auth {
                Auth::UserPassword(u, p) => {
                    write!(f, "{}://{}:{}@{}:{}", self.proto, u, p, self.ip, self.port)
                }
            }
        } else {
            write!(f, "{}://{}:{}", self.proto, self.ip, self.port)
        }
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
pub struct IgnoreSubnet {
    pub cidr: Ipv4Cidr,
    pub port: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxycConfig {
    #[serde(rename = "proxy", deserialize_with = "seq_string_or_struct")]
    pub proxies: Vec<ProxyConf>,
    pub chain_type: ChainType,
    #[serde(with = "LevelFilterRef")]
    pub log_level: LevelFilter,
    #[serde(default = "default_tcp_read")]
    pub tcp_read_timeout: usize,
    #[serde(default = "default_tcp_connect")]
    pub tcp_connect_timeout: usize,
    pub proxy_dns: bool,
    pub dns_subnet: u8,
    pub ignore_subnets: Vec<IgnoreSubnet>,
}

impl ProxycConfig {
    pub fn new(path: &Path) -> Result<Self, ConfigError> {
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

impl Default for ProxycConfig {
    fn default() -> Self {
        Self {
            proxies: vec![],
            chain_type: ChainType::Strict,
            log_level: LevelFilter::Info,
            tcp_read_timeout: 15000,
            tcp_connect_timeout: 8000,
            proxy_dns: true,
            dns_subnet: 224,
            ignore_subnets: vec![],
        }
    }
}

/// Either deserializes a vec of structs or a vec of strings.
fn seq_string_or_struct<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de> + FromStr<Err = ConfigError>,
    D: Deserializer<'de>,
{
    struct StringOrStruct<T>(PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for StringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = ConfigError>,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or map")
        }

        fn visit_str<E>(self, value: &str) -> Result<T, E>
        where
            E: de::Error,
        {
            FromStr::from_str(value).map_err(de::Error::custom)
        }

        fn visit_map<M>(self, map: M) -> Result<T, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }

    // This is a common trick that enables passing a Visitor to the
    // `seq.next_element` call below.
    impl<'de, T> DeserializeSeed<'de> for StringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = ConfigError>,
    {
        type Value = T;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }
    }

    struct SeqStringOrStruct<T>(PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for SeqStringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = ConfigError>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("sequence of strings or maps")
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            // Tell it which Visitor to use by passing one in.
            while let Some(element) = seq.next_element_seed(StringOrStruct(PhantomData))? {
                vec.push(element);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_seq(SeqStringOrStruct(PhantomData))
}
