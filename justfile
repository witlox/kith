# Kith development tasks

default:
    @just --list

check: fmt clippy test

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

build:
    cargo build --workspace

release:
    cargo build --workspace --release

profile name scope="":
    ./switch-profile.sh {{name}} {{scope}}

proto:
    cargo build -p kith-daemon --features codegen
