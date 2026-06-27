# Redis Adapter

Redis compatibility is not part of the Cachebox MVP.

Cachebox should be built around the native socket cache API first. A Redis adapter
may be useful later for adoption, but it must not shape the cache engine or force
Redis data-structure semantics into the core design.

## Why It Is Deferred

- Redis clients expect Redis behavior, not just a wire protocol.
- Partial compatibility creates confusing application failures.
- RESP support would add parser, encoder, command, and edge-case work before the
  cache-native product exists.
- Cachebox needs first-class concepts such as leases, stale values, tags,
  namespaces, and cost hints.

## Adapter Rules If Added Later

- Keep it as a separate transport layer over the same cache engine.
- Support only commands backed by native cache semantics.
- Document unsupported commands clearly.
- Test behavior against real Redis clients.
- Do not add engine features solely to imitate Redis corner cases.

## Possible Minimal Command Set

A future adapter could start with:

- `PING`
- `GET`
- `SET`
- `DEL`
- `EXISTS`
- `EXPIRE`
- `TTL`
- `MGET`
- `MSET`

That list is intentionally tentative. The adapter should be justified by real
adoption needs, not by compatibility for its own sake.
