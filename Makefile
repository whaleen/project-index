.PHONY: install dev

install:
	cargo install --path cli

dev:
	cargo run --manifest-path cli/Cargo.toml --bin px
