# Project justfile — common development commands.

# Format the workspace
lint-fix:
    cargo +nightly fmt --all

# Check format and lint for the workspace.
lint:
    cargo +nightly fmt --all -- --check
    cargo clippy --workspace -- -D warnings

# Type-check the entire workspace.
check:
    cargo check --workspace

# Run all workspace tests.
test:
    cargo test --workspace

# Run the in-process discrete-event simulation (configure via .env).
sim:
    cargo run --release --bin compare

# Local generation defaults — override on CLI: just local_nodes=10 local-gen
local_nodes    := "5"
local_strategy := "pedersen"
local_output   := "/tmp/rlnc"

# Generate local node configs.
local-gen:
    cargo run --release --bin deploy -- generate local \
        --nodes {{local_nodes}} \
        --strategy {{local_strategy}} \
        --block-size {{block_size}} \
        --mesh-degree {{mesh_degree}} \
        --num-blocks {{num_blocks}} \
        --seed {{seed}} \
        --output {{local_output}}

# Build Docker image for the node binary.
docker-build:
    docker build -t rlnc-node:latest .

# Docker generation defaults — override on CLI: just nodes=10 docker-gen
nodes             := "5"
strategy          := "pedersen"
block_size        := "2000000"
mesh_degree       := "8"
num_blocks        := "5"
block_interval    := "5"
startup_delay     := "10"
p2p_port          := "3000"
metrics_port      := "9090"
seed              := "42"
output            := "./docker"

# Generate Docker Compose configs.
docker-gen:
    cargo run --release --bin deploy -- generate docker \
        --nodes {{nodes}} \
        --strategy {{strategy}} \
        --block-size {{block_size}} \
        --mesh-degree {{mesh_degree}} \
        --num-blocks {{num_blocks}} \
        --block-interval-secs {{block_interval}} \
        --startup-delay-secs {{startup_delay}} \
        --p2p-port {{p2p_port}} \
        --metrics-port {{metrics_port}} \
        --seed {{seed}} \
        --output {{output}} \
        --monitoring

# Run the Docker Compose network.
docker-up: docker-build
    cd docker && docker compose up

# Run with monitoring (Prometheus).
docker-up-monitoring: docker-build
    cd docker && docker compose --profile monitoring up

# Tear down all containers (including monitoring profile) and remove the network.
docker-down:
    cd docker && docker compose --profile monitoring down --remove-orphans
