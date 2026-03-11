# Stage 1: Build the node binary.
# Must track commonware's effective MSRV — currently ≥1.93 due to transitive deps
# (crc-fast, sysinfo) and stabilized std APIs (duration_constructors_lite).
FROM rust:1.93-bookworm AS builder

# Isolated build directory inside the container.
WORKDIR /build

# Copy workspace manifests first, then source — allows Docker layer caching
# to skip `cargo build` when only non-Cargo files change.
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Compile only the node binary in release mode (strips debug info, enables optimizations).
RUN cargo build --release --bin node

# Stage 2: Minimal runtime image (no Rust toolchain).
FROM debian:bookworm-slim

# TLS root certs needed for any outbound HTTPS (e.g. metrics push).
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && rm -rf /var/lib/apt/lists/*

# Pull the compiled binary from the builder stage into the runtime image.
COPY --from=builder /build/target/release/node /usr/local/bin/node

# Default data directory — node configs mount volumes here.
RUN mkdir -p /data

# Container runs the node binary; CLI args are appended via `command:` in docker-compose.
ENTRYPOINT ["node"]
