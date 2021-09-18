.PHONY: all clean
# We can't yet depend on a cdylib:
# https://github.com/rust-lang/cargo/issues/8311
# https://github.com/rust-lang/cargo/issues/7825
# So let's build crates manually.
all:
	cargo build -p libproxyc
	cargo build

release:
	cargo build -p libproxyc --release
	cargo build --release

clippy:
	cargo clippy
	cargo clippy -p libproxyc

clean:
	cargo clean -p libproxyc
	cargo clean
