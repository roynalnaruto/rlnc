# Running the Node Binary

## Step 1 — Generate configs with the `deploy` binary

```bash
cargo run --bin deploy -- generate local \
  --nodes 5 \
  --strategy pedersen \
  --output /tmp/rlnc
```

Available flags:

| Flag | Default | Description |
|:-----|:--------|:------------|
| `--nodes` | 5 | Number of nodes (node 0 is proposer) |
| `--strategy` | `pedersen` | `baseline`, `pedersen`, or `bfkw` |
| `--block-size` | 2000000 | Block size in bytes |
| `--mesh-degree` | 8 | Peers to forward to |
| `--num-blocks` | 5 | Blocks to propagate |
| `--start-port` | 3000 | First port (increments by 2 per node) |
| `--output` | `/tmp/rlnc` | Output directory |
| `--seed` | 42 | Deterministic RNG seed |

This creates:
- **`/tmp/rlnc/peers.yaml`** — peer list (Ed25519 pubkeys + addresses)
- **`/tmp/rlnc/node-0.yaml`** through **`node-4.yaml`** — per-node configs with keys, ports, strategy, and role

## Step 2 — Start each node in a separate terminal

```bash
# Terminal 1 (proposer)
cargo run --release --bin node -- \
  --config /tmp/rlnc/node-0.yaml \
  --peers /tmp/rlnc/peers.yaml

# Terminal 2
cargo run --release --bin node -- \
  --config /tmp/rlnc/node-1.yaml \
  --peers /tmp/rlnc/peers.yaml

# Terminal 3-5: same pattern with node-2, node-3, node-4
```

Node 0 waits `startup_delay_secs` (default 10s) for peers to connect, then starts proposing blocks. Receivers decode and forward automatically.

## Logging

Control verbosity via `RUST_LOG`:

```bash
RUST_LOG=info cargo run --release --bin node -- --config ...   # default
RUST_LOG=debug cargo run --release --bin node -- --config ...  # per-node decode events
RUST_LOG=trace cargo run --release --bin node -- --config ...  # full pipeline visibility
```

## Quick local test (all 3 strategies)

```bash
for strat in baseline pedersen bfkw; do
  cargo run --bin deploy -- generate local \
    --nodes 5 --strategy $strat --output /tmp/rlnc-$strat
done
```

Then launch each set of 5 nodes in separate terminals.
