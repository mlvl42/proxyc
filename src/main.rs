use anyhow::{anyhow, Result};
use log::LevelFilter;
use proxyc_common::{ChainType, ProxyConf, ProxycConfig};
use std::os::unix::process::CommandExt;
use std::process::Command;
use structopt::clap::AppSettings;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(
    name = "proxyc",
    about = "proxychains something something",
    setting = AppSettings::TrailingVarArg
)]
struct ProxycOpt {
    /// proxy definition
    #[structopt(short, long)]
    proxy: Vec<ProxyConf>,

    /// log level
    #[structopt(rename_all = "lowercase", short, long)]
    log_level: Option<LevelFilter>,

    /// chain type
    #[structopt(short, long)]
    chain: Option<ChainType>,

    /// the command line to hook
    args: Vec<String>,
}

const CONFIG_FILE_PATHS: [&str; 3] = ["./proxyc.toml", "~/proxyc.toml", "/etc/proxyc/proxyc.toml"];

fn main() -> Result<()> {
    let opts = ProxycOpt::from_args();

    let program = opts.args.iter().next();
    let args = opts.args.iter().skip(1);

    // TODO check provided conf file

    // no files provided, try to find one
    let config_path = CONFIG_FILE_PATHS
        .iter()
        .find(|x| std::fs::metadata(x).is_ok())
        .map(|x| std::fs::canonicalize(x).ok())
        .and_then(|x| x)
        .ok_or(anyhow!("proxyc.toml file not found"))?;

    // parse the config before passing it down the shared library through the
    // environment
    let config = {
        let mut config = ProxycConfig::new(&config_path)
            .map_err(|e| anyhow!("invalid configuration: {:?}", e))?;

        // enrich config with values provided in command line
        if opts.proxy.len() > 0 {
            config.proxies = opts.proxy;
        }

        if let Some(level) = opts.log_level {
            config.log_level = level;
        }

        if let Some(chain) = opts.chain {
            config.chain_type = chain;
        }

        config
    };

    // pass config in env variable
    let config_env = config.to_json()?;

    // TODO
    // - get .so dynamically ?
    Ok(match program {
        Some(x) => {
            Command::new(&x)
                .args(args)
                .env(
                    "LD_PRELOAD",
                    "/home/jed/projects/proxyc/target/debug/libproxyc.so",
                )
                .env("PROXYC_CONFIG", config_env)
                .exec();
        }
        None => {
            ProxycOpt::clap().print_help().unwrap();
            println!();
            std::process::exit(1);
        }
    })
}
