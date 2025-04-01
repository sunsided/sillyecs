[private]
help:
    @just --list --unsorted

# Format the code
fmt:
    cargo fmt --all

# Lint the code and apply fixes
lint *ARGS:
    cargo clippy --fix --allow-staged {{ ARGS }}
    cargo fmt --all

# Build the workspace
build:
    cargo build

# Test all targets.
test *ARGS:
    cargo test {{ARGS}}

# Just build the documentation.
build-docs:
    export DOCSRS=1
    cargo +nightly doc --no-deps --workspace --all-features --document-private-items

[private]
build-doc: build-docs

# Build and open the documentation.
docs:
    export DOCSRS=1
    cargo +nightly doc --no-deps --workspace --all-features --document-private-items --open

[private]
doc: docs

[private]
open-docs: docs
