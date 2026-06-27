# Cachebox Documentation

Cachebox is a self-hosted cache server with a native socket data plane,
raw-byte values, cache metadata, stampede protection, tag invalidation, bounded
memory, metrics, and AI-oriented helper utilities.

Start here:

- [Quickstart](quickstart.md): run the server and make first native client
  requests.
- [Usage Guide](usage.md): examples for keys, TTLs, stale values, batch reads,
  tags, leases, memory limits, metrics, and errors.
- [Native Sockets](native-sockets.md): start TCP and Unix socket listeners and
  use the binary codec from Rust.
- [AI Helpers](ai-helpers.md): prompt keys, embedding keys, generation leases,
  stream capture, and cost metadata.
- [Benchmarks](benchmarks.md): benchmark command, scenarios, and current local
  baseline.

Project planning notes, architecture sketches, historical checkpoints, and
implementation prompts live in [internal/](internal/).
