# Cachebox Agent Notes

Cachebox is a Rust 2024 project. Keep changes small, direct, and consistent
with the existing single-binary architecture.

## Development Commands

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Run the spawned-binary smoke tests when changing listener startup, shutdown,
protocol transport, or client/server process behavior:

```sh
cargo test --test spawned_client -- --ignored
```

## Release Rules

Commits and PR titles must use Conventional Commits. Use `fix:` for bug fixes,
`feat:` for new user-visible behavior, `perf:` for performance improvements,
and `docs:` for documentation-only changes.

Do not edit generated release metadata manually unless fixing release automation.
Version bumps, changelog updates, SemVer tags, GitHub Releases, binary assets,
and GHCR images are handled by GitHub Actions.

## Container Notes

The repository Docker image is built from `rust:1-trixie` and runs on
`gcr.io/distroless/cc-debian13:nonroot`. Keep the runtime image focused on
running the `cachebox` binary; do not add development tools to the runtime
stage.
