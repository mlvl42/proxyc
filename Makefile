.PHONY: all dev clean install tests

all:
	cargo build --release

dev:
	cargo build

clean:
	cargo clean

install:
	strip --strip-all target/release/proxyc
	strip --strip-all target/release/libproxyc.so
	install -Dm 755 -t /usr/local/bin target/release/proxyc
	install -Dm 755 -t /usr/local/lib target/release/libproxyc.so

tests:
	./tests/e2e/tests.sh
