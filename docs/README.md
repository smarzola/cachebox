# Cachebox Documentation

Cachebox is a small native-socket cache server for raw bytes, TTLs, stale
reads, tag invalidation, lease-based stampede protection, bounded memory, and
metrics.

Read these in order if you are new to the project:

- [Quickstart](quickstart.md): build the binary, start the listeners, and make
  first client requests.
- [Usage Guide](usage.md): how to choose keys, TTLs, stale windows, tags,
  leases, batch reads, pipelining, memory limits, and metrics.
- [Native Sockets](native-sockets.md): when to use TCP or Unix sockets and how
  persistent connections behave.
- [Protocol Specification](protocol.md): the wire format for clients that
  implement the native protocol directly.
- [Internals And Performance](internals.md): memory structures, eviction,
  expiration, tag routing, pipelined execution, and the performance
  optimizations in the current server.
- [AI Helpers](ai-helpers.md): deterministic prompt and embedding keys,
  generation lease flows, stream capture, and cost metadata.
- [Benchmarks](benchmarks.md): local benchmark scenarios and current output.

Planning notes, development checkpoints, and implementation prompts live in
[internal/](internal/). They are useful for development context, but user-facing
behavior is documented in the top-level files listed above.
