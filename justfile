set shell := ["bash", "-cu"]

default:
    @just --list

dev:
    cargo watch -x 'run --bin imkitchen -- serve'

run:
    cargo run --bin imkitchen -- serve

build:
    cargo build --release

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

check: fmt-check lint test

machete:
    cargo machete

db-reset:
    rm -f imkitchen.db imkitchen.db-shm imkitchen.db-wal
