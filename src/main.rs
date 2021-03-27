use anyhow::{anyhow, Result};
use proxyc::config::ProxycConfig;
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
    /// a sample option
    #[structopt(short, long)]
    fixme: Option<String>,

    /// hook me bebprogram to hook
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

    println!("config: {:?}", config_path);

    // try to parse the config before actually passing it down the shared
    // library.
    let config =
        ProxycConfig::new(&config_path).map_err(|e| anyhow!("invalid configuration: {:?}", e))?;
    println!("{:?}", config);

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
                .exec();
        }
        None => {
            ProxycOpt::clap().print_help().unwrap();
            println!();
            std::process::exit(1);
        }
    })
}
