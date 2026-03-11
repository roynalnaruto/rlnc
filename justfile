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
