# Kith development tasks

default:
    @just --list

# Run all checks (what CI does)
check: fmt clippy test

# Format check
fmt:
    cargo fmt --all -- --check

# Lint
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests
test:
    cargo test --workspace --exclude kith-acceptance
    cargo test -p kith-acceptance

# Fast unit tests only
test-unit:
    cargo test --workspace --exclude kith-acceptance --exclude kith-e2e

# E2e tests
test-e2e:
    cargo test -p kith-e2e

# BDD acceptance tests
test-bdd:
    cargo test -p kith-acceptance

# Container tests (requires docker)
test-containers:
    docker build -t kith-daemon .
    cargo test -p kith-e2e --features containers --test containers

# Build workspace
build:
    cargo build --workspace

# Build release binaries
release:
    cargo build --release -p kith-shell --bin kith
    cargo build --release -p kith-daemon --bin kith-daemon

# Build with all optional features
build-full:
    cargo build -p kith-mesh --features nostr,wireguard
    cargo build -p kith-shell --bin kith
    cargo build -p kith-daemon --bin kith-daemon

# Build docker image
docker:
    docker build -t kith-daemon .
