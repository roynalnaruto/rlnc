# Random Linear Network Coding

A proof-of-concept comparing bandwidth and compute costs of three
block-propagation strategies in peer-to-peer networks.

> **Warning:** This is a personal research PoC — not production software.
> APIs, wire formats, and simulation parameters will change without notice.

## Overview

This repository implements and benchmarks three approaches to block propagation
over random-mesh topologies: naive gossipsub-style full-block forwarding,
RLNC with Pedersen commitment verification, and RLNC with BFKW linearly
homomorphic signatures. Both a discrete-event simulation engine and a live p2p
networking mode are included so that strategies can be compared under controlled
and realistic conditions.

Based on two ethresear.ch proposals:
- [Faster block/blob propagation in Ethereum](https://ethresear.ch/t/faster-block-blob-propagation-in-ethereum/21370)
- [Linearly homomorphic signatures for RLNC](https://ethresear.ch/t/linearly-homomorphic-signatures-for-rlnc/24072)

## Strategies

| Crate | Strategy | Description |
|:------|:---------|:------------|
| `p2p-pedersen` | RLNC + Pedersen Commitments | Coded chunks verified with N Pedersen commitments per packet. |
| `p2p-bfkw` | RLNC + BFKW Signatures | Coded chunks verified with a single BFKW signature per packet. |

## Protocol Summaries

### Pedersen Commitments for RLNC

The proposer splits a block into N chunks and commits to each with a Pedersen
commitment (a multi-scalar multiplication over BLS12-381 G1). Every coded
packet carries all N commitments plus a BLS signature over them. Intermediate
nodes verify that the commitment to the received coded chunk equals the linear
combination of the original commitments — a property that holds by linearity of
MSM. Since the commitments are invariant under re-encoding, the same proof is
simply cloned when forwarding.

### BFKW Linearly Homomorphic Signatures

The BFKW scheme uses the "basis vector trick" to bind both the coded data and
its encoding coefficients into a single G2 signature via MSM over hash-to-G2
points derived from the block identifier. Verification requires a pairing check
rather than an MSM equality check. Crucially, the signatures are truly
homomorphic: a valid signature for a re-encoded chunk is obtained by taking an
MSM of the existing signatures with the combining coefficients. This yields
lower per-packet bandwidth (one 96-byte G2 point vs N G1 points) at the cost
of more expensive verification (two pairings per packet).

See [`docs/book.md`](docs/book.md) for the full technical deep-dive, including 
worked math and diagrams.

## Getting Started

### Prerequisites

- **Rust 1.93+** (stable) and a **nightly** toolchain (for `rustfmt`)
- **[`just`](https://github.com/casey/just)** task runner
- **Docker** (optional, for containerized p2p mode)

### Simulation Mode

Run the in-process discrete-event simulation (configure parameters via `.env`):

```sh
just sim
```

### Local P2P Mode

Generate local node configs, then launch the nodes:

```sh
just local-gen
```

See [`docs/instructions.md`](docs/instructions.md) for how to launch nodes
after generation.

### Docker Mode

Build and launch a containerized network:

```sh
just docker-gen
just docker-up

# launch with prometheus monitoring
just docker-up-monitoring
```

See [`docs/instructions.md`](docs/instructions.md) for full deployment
documentation including monitoring setup.

## Development Commands

```sh
just lint-fix   # Format the workspace (nightly rustfmt)
just lint       # Check formatting + clippy lints
just check      # Type-check the entire workspace
just test       # Run all workspace tests
just docs       # Build docs (warnings-as-errors) + run doc-tests
```

## Credits & Acknowledgements

- Based on proposals by [@potuz](https://github.com/potuz):
  [RLNC](https://ethresear.ch/t/faster-block-blob-propagation-in-ethereum/21370)
  and
  [LHSS](https://ethresear.ch/t/linearly-homomorphic-signatures-for-rlnc/24072)
- Reference PoC (Pedersen over Ristretto255):
  [potuz/rlnc_poc](https://github.com/potuz/rlnc_poc)
- Built extensively with
  [Commonware](https://github.com/commonwarexyz/monorepo) libraries
- BFKW construction:
  [Boneh–Freeman–Katz–Waters (2008)](https://eprint.iacr.org/2008/316)
