# Cachebox Rust Client

This crate is the official Rust client for Cachebox. It uses
`cachebox-protocol` for native frame encoding and decoding.

## Local Development

Run client-focused tests:

```sh
cargo test -p cachebox-client
```

Run client clippy checks:

```sh
cargo clippy -p cachebox-client --all-targets -- -D warnings
```

Run the full workspace checks before opening a PR:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --test spawned_client -- --ignored
uv run --with pytest --with clients/python pytest clients/python/tests
```
