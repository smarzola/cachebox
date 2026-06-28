# Contributing

Cachebox uses semantic commits and automated releases. Keep pull requests small
and make the intended release impact clear from the commit messages.

## Commit Messages

Every commit must follow the Conventional Commits format:

```text
type(optional-scope): short description
```

Release-producing types:

- `fix:` releases a patch version.
- `perf:` releases a patch version.
- `feat:` releases a minor version.
- `type!:` or a `BREAKING CHANGE:` footer releases a major version.

Non-release types:

- `docs:`
- `test:`
- `ci:`
- `build:`
- `chore:`
- `style:`
- `refactor:`
- `revert:`

Examples:

```text
feat(protocol): add batch delete
fix(server): close idle native sockets
docs: clarify cache TTL behavior
```

Use `fix:` instead of `bugfix:`.

## Pull Requests

PR titles must also use Conventional Commits. Prefer squash merging so the final
merge commit has one clear semantic title.

The CI gate checks:

- commit message format
- PR title format
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

## Releases

Releases are automated through generated release PRs.

When a normal PR with release-producing commits merges to `main`, release-plz
opens or updates a release PR with the version and changelog changes. Merging
that generated release PR publishes the SemVer tag, GitHub Release, release
binaries, and GHCR image.

Documentation-only and other non-release commits do not create a release.
Direct pushes to `main` do not run the release workflow.

Release binaries are built for:

- Linux x64: `x86_64-unknown-linux-gnu`
- Linux ARM64: `aarch64-unknown-linux-gnu`
- macOS Apple Silicon: `aarch64-apple-darwin`

Container images are published to:

```text
ghcr.io/smarzola/cachebox
```

The Docker image uses `rust:1-trixie` for builds and
`gcr.io/distroless/cc-debian13:nonroot` for runtime.

## Local Checks

Run these before opening a PR:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
