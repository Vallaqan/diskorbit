# DiskOrbit task runner — run `just` to list all tasks.
# Install Just: https://github.com/casey/just

# List available tasks
default:
    @just --list

# ── Build ──────────────────────────────────────────────────────────────────────

# Build a debug binary
build:
    cargo build

# Build an optimized release binary
release:
    cargo build --release

# ── Run ────────────────────────────────────────────────────────────────────────

# Run in debug mode
run:
    cargo run

# Run with release optimizations
run-release:
    cargo run --release

# ── Quality ────────────────────────────────────────────────────────────────────

# Check code formatting (mirrors `cargo lint` alias)
lint:
    cargo lint

# Apply code formatting
fmt:
    cargo fmt

# Run Clippy linter (warnings are errors)
clippy:
    cargo clippy -- -D warnings

# Run unit tests
test:
    cargo test

# Run all CI checks: lint + clippy + test
check: lint clippy test

# ── Utilities ──────────────────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean

# Show dependency tree
deps:
    cargo tree

# Check for outdated dependencies (requires cargo-outdated)
outdated:
    cargo outdated

# Audit dependencies for security advisories (requires cargo-audit)
audit:
    cargo audit
