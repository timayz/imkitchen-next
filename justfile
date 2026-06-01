set shell := ["bash", "-cu"]

default:
    @just --list

css-web:
    tailwindcss -i web/styles/app.css -o web/static/app.css --minify

css-web-watch:
    tailwindcss -i web/styles/app.css -o web/static/app.css --watch

css-admin:
    tailwindcss -i admin/styles/app.css -o admin/static/app.css --minify

css-admin-watch:
    tailwindcss -i admin/styles/app.css -o admin/static/app.css --watch

css: css-web css-admin

dev: css
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT INT TERM
    just css-web-watch &
    just css-admin-watch &
    cargo watch -x 'run --bin imkitchen -- serve'

run: css
    cargo run --bin imkitchen -- serve

build: css
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
