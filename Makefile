PREFIX ?= $(HOME)/.local
BIN := $(PREFIX)/bin
APPS := $(PREFIX)/share/applications

.PHONY: build run test lint install uninstall windows release bench

build:
	cargo build --release

run:
	cargo run --release -p dirstat-app

test:
	cargo test --workspace

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

ICONS := $(PREFIX)/share/icons/hicolor/256x256/apps

install: build
	install -Dm755 target/release/dirstat $(BIN)/dirstat
	install -Dm644 packaging/dirstat.desktop $(APPS)/dirstat.desktop
	install -Dm644 crates/dirstat-app/assets/icon.png $(ICONS)/dirstat.png
	-update-desktop-database $(APPS) 2>/dev/null

uninstall:
	rm -f $(BIN)/dirstat $(APPS)/dirstat.desktop $(ICONS)/dirstat.png

windows:
	cargo zigbuild --release --target x86_64-pc-windows-gnu -p dirstat-app

release:
	@test -n "$(VERSION)" || (echo "usage: make release VERSION=1.2.3" && exit 1)
	@echo "$(VERSION)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$$' || (echo "VERSION must look like 1.2.3 or 1.2.3-beta.1" && exit 1)
	@test -z "$$(git status --porcelain)" || (echo "commit or stash your changes first" && exit 1)
	@! git rev-parse -q --verify "refs/tags/v$(VERSION)" >/dev/null || (echo "tag v$(VERSION) already exists" && exit 1)
	perl -0pi -e 's/^version = "[^"]*"/version = "$(VERSION)"/m' Cargo.toml
	cargo update --workspace
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo test --workspace --locked
	git diff --quiet || git commit -am "chore: release v$(VERSION)"
	git tag -a "v$(VERSION)" -m "v$(VERSION)"
	@echo "Ready. Publish with: git push origin HEAD v$(VERSION)"

bench:
	cargo run --release -p dirstat-core --example scan_bench -- $(or $(DIR),/usr)
	cargo run --release -p dirstat-treemap --example layout_bench -- $(or $(DIR),/usr)
