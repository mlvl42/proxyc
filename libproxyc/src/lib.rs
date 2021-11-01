#[macro_use]
extern crate log;
extern crate pretty_env_logger;

mod core;
mod error;
mod hook;
mod proxy;
mod util;

/// This is called when our dynamic library is loaded, so we setup our internals
/// here.
#[no_mangle]
#[link_section = ".init_array"]
static LD_PRELOAD_INIT: extern "C" fn() = self::init;
extern "C" fn init() {
    let config = &*core::CONFIG;
    std::env::set_var("RUST_LOG", config.log_level.to_string());

    pretty_env_logger::init();

    info!("chain_type: {:?}", config.chain_type);
    info!("proxies:");
    for p in &config.proxies {
        info!("\t{}", p);
    }
}
