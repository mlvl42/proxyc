# proxyc

`proxyc` is a rust proxy chaining command line tool.

Simply prepend your commands with this utility, define a list of SOCKS, HTTP or
raw proxies you wish to use and watch all the network traffic be automatically
relayed through each proxies. This kind of tool is very useful during internal
penetration tests in order to bounce

This program hooks the libc network functions by injecting a shared library via
LD_PRELOAD. It is heavily inspired by https://github.com/rofl0r/proxychains-ng.

> **WARNINGS**:
> I am writing and building this project in order to learn Rust, bugs and
> issues are expected to occur. Should you want to use a proxy chaining utility
> for serious activities, consider using [one](https://github.com/haad/proxychains)
> of the [popular](https://github.com/rofl0r/proxychains-ng) choices.
>
> Moreover, remember that this project only works with dynamically linked
> programs.

## Installation

FIXME: cargo install is just for binary crates, however our program also needs
the libproxyc.so to work.

## Usage

```
$ proxyc curl "https://ipinfo.io/what-is-my-ip"
```

`proxyc` searches for a valid configuration file in the following paths:

```
- ./proxyc.toml
- ~/proxyc.toml
- /etc/proxyc/proxyc.toml
```

However, all the configuration options can be specified as command line
arguments. For instance, the list of proxies to use can be expressed this way:

```
$ proxyc --proxy "socks5://127.0.0.1:1080" --proxy "socks4://127.0.0.1:1081" smbclient.py 'test.local/user:pass@SHARE'
# or comma separated
$ proxyc -p "socks5://127.0.0.1:1080,socks4://127.0.0.1:1081" smbclient.py 'test.local/user:pass@SHARE'
```

## Sample configuration

```toml
# defines the verbosity: off, trace, debug, info, warn of error
log_level = "debug"

# connect calls matching this range won't be proxied.
#[[ignore_subnets]]
#cidr = "128.0.0.0/24"

# whether dns should be proxied or not.
proxy_dns = true

# if the proxified application issues a DNS request, we return an IP address
# from this range.
#dns_subnet = 224

# list of available proxies
proxy = [
	"socks5://127.0.0.1:1080",
]

# how the list of proxies should be treated.
# strict: connect successively through each proxies (default).
# dynamic: not implemented.
# random:  not implemented.
chain_type = "strict"

# examples with more options
# available protocols: raw, http, https, socks4, socks5
#proxy = [
#  "socks4://1.2.3.4:4242",
#  "socks5://user:pass@1.1.1.1:1081",
#  "http://1.1.1.1:80",
#  "raw://1.1.1.1:80",
#]

# alternate way of defining a list of proxies
#[[proxy]]
#type = "socks5"
#ip = "127.0.0.1"
#port = 1080
#auth = { UserPassword = { 0 = "username", 1 = "password" } }
```
