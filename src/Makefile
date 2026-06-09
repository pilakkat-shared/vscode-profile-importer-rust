RUSTFLAGS =

.PHONY: all build release clean dist

all: build

build:
	cargo build

release:
	cargo build --release

dist: release
	# create a simple tarball with the release binary
	mkdir -p dist
	cp target/release/vscode-profile-importer dist/
	tar -C dist -czf vscode-profile-importer.tar.gz vscode-profile-importer

clean:
	cargo clean
	rm -rf dist
