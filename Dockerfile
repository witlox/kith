# Multi-stage build for kith-daemon container
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
RUN cargo build --release -p kith-daemon --bin kith-daemon

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/kith-daemon /usr/local/bin/kith-daemon

ENV KITH_LISTEN_ADDR=0.0.0.0:9443
ENV KITH_MACHINE_NAME=kith-node
ENV KITH_TOFU=true

EXPOSE 9443
ENTRYPOINT ["kith-daemon"]
