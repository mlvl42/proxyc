use anyhow::{anyhow, bail, Context, Result};
use log::LevelFilter;
use proxyc_common::{ChainType, ProxyConf, ProxycConfig};
use std::env;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use structopt::clap::AppSettings;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(
    name = "proxyc",
    about = "proxy chaining tool",
    setting = AppSettings::TrailingVarArg
)]
struct ProxycOpt {
    /// proxy definition
    #[structopt(short, long, require_delimiter = true)]
    proxy: Vec<ProxyConf>,

    /// log level
    #[structopt(rename_all = "lowercase", short, long)]
    log_level: Option<LevelFilter>,

    /// suppress output (same as --log-level off)
    #[structopt(short, long)]
    quiet: bool,

    /// chain type
    #[structopt(short, long)]
    chain: Option<ChainType>,

    /// custom path to config file
    #[structopt(short, long, parse(from_os_str))]
    file_config: Option<PathBuf>,

    /// read timeout
    #[structopt(long)]
    tcp_read_timeout: Option<usize>,

    /// connect timeout
    #[structopt(long)]
    tcp_connect_timeout: Option<usize>,

    /// the command line to hook
    args: Vec<String>,
}

const CONFIG_FILE_PATHS: [&str; 3] = ["./proxyc.toml", "~/proxyc.toml", "/etc/proxyc/proxyc.toml"];

// search the debug libproxyc.so in the current directory if proxyc is compiled
// in debug profile.
// This allows "cargo run" to work and eases testing.
#[cfg(debug_assertions)]
const SHARED_LIB_PATHS: [&str; 2] = ["./target/debug/libproxyc.so", "/usr/lib/libproxyc.so"];
#[cfg(not(debug_assertions))]
const SHARED_LIB_PATHS: [&str; 1] = ["/usr/lib/libproxyc.so"];

fn main() -> Result<()> {
    let opts = ProxycOpt::from_args();

    let program = opts.args.get(0);
    let args = opts.args.iter().skip(1);

    // find libproxyc.so
    let lib_path = SHARED_LIB_PATHS
        .iter()
        .find(|x| std::fs::metadata(x).is_ok())
        .map(|x| std::fs::canonicalize(x).ok())
        .and_then(|x| x)
        .ok_or_else(|| anyhow!("libproxyc.so not found"))?
        .display()
        .to_string();

    // no files provided, try to find one
    let config_path = match opts.file_config {
        Some(p) => Some(p),
        None => CONFIG_FILE_PATHS
            .iter()
            .find(|x| std::fs::metadata(x).is_ok())
            .map(|x| std::fs::canonicalize(x).ok())
            .and_then(|x| x),
    };

    // parse the config before passing it down the shared library through the
    // environment
    let config = {
        let mut config = {
            if let Some(p) = &config_path {
                ProxycConfig::new(p)
                    .with_context(|| format!("Invalid configuration file: {:?}", config_path))?
            } else {
                ProxycConfig::default()
            }
        };
        // providing proxies in CLI parameters overwrites the proxies defined
        // in the configuration file, if any.
        if !opts.proxy.is_empty() {
            config.proxies = opts.proxy;
        }

        if opts.quiet {
            config.log_level = LevelFilter::Off;
        } else if let Some(level) = opts.log_level {
            config.log_level = level;
        }

        if let Some(chain) = opts.chain {
            config.chain_type = chain;
        }

        if let Some(tcp_connect_timeout) = opts.tcp_connect_timeout {
            config.tcp_connect_timeout = tcp_connect_timeout;
        }

        if let Some(tcp_read_timeout) = opts.tcp_read_timeout {
            config.tcp_read_timeout = tcp_read_timeout;
        }

        config
    };

    // check if there are any proxies defined
    if config.proxies.is_empty() {
        bail!("at least one proxy is required, use --proxy or define the list of proxies in the configuration file.");
    }

    // pass config in env variable
    let config_env = config.to_json()?;

    // do not overwrite LD_PRELOAD variable if it is already set
    let ld_preload = match env::var("LD_PRELOAD") {
        Ok(val) => format!("{}:{}", val, lib_path),
        Err(_e) => lib_path,
    };

    match program {
        Some(x) => {
            Command::new(&x)
                .args(args)
                .env("LD_PRELOAD", ld_preload)
                .env("PROXYC_CONFIG", config_env)
                .exec();
        }
        None => {
            ProxycOpt::clap().print_help().unwrap();
            println!();
            std::process::exit(1);
        }
    };
    Ok(())
}
