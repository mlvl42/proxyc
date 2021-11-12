#[macro_use]
extern crate log;
extern crate pretty_env_logger;

mod core;
mod error;
mod hook;
mod proxy;
mod util;

static ONCE: std::sync::Once = std::sync::Once::new();
/// This is called when our dynamic library is loaded, so we setup our internals
/// here.
#[no_mangle]
#[link_section = ".init_array"]
static LD_PRELOAD_INIT: extern "C" fn() = self::init;
extern "C" fn init() {
    ONCE.call_once(|| {
        let config = &*core::CONFIG;
        std::env::set_var("RUST_LOG", config.log_level.to_string());
        pretty_env_logger::init();
        debug!("init pid: {}", std::process::id());
        info!("chain_type: {:?}", config.chain_type);
        info!("proxies:");
        for p in &config.proxies {
            info!("\t{}", p);
        }
    });
}
