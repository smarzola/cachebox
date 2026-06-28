# Performance Uplift Borrowed Decode

Milestone 1 adds a borrowed native request decode path for hot commands. It
keeps the public owned protocol decode and client APIs intact.

## Change

The server now attempts borrowed decode before falling back to the owned request
decoder:

- `Get`
- `Delete`
- `TagInvalidate`
- `LeaseStart`

Borrowed decode validates the same namespace, tag, frame, and lease TTL rules,
but returns string and byte slices into the connection frame buffer instead of
allocating owned `String` and `Vec` values. `Put`, `BatchGet`, and
`LeaseComplete` still use the owned decoder.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 24042 | 23166 | -3.6% |
| `lease_contention` | `loopback_tcp` | 25500 | 25458 | -0.2% |
| `tag_invalidate_empty` | `loopback_tcp` | 23250 | 23000 | -1.1% |
| `single_key_get` | `loopback_unix` | 14125 | 14083 | -0.3% |
| `lease_contention` | `loopback_unix` | 14333 | 13958 | -2.6% |
| `tag_invalidate_empty` | `loopback_unix` | 13791 | 13208 | -4.2% |
| `concurrent_get_16` | `loopback_tcp` | 101416 | 100458 | -0.9% |
| `concurrent_get_16` | `loopback_unix` | 71541 | 70000 | -2.2% |

The gains are real but small. Borrowed decode removes owned request-field
allocation for targeted commands, but the hot read path still clones returned
values, takes the global engine mutex, updates atomics, and performs socket
read/write syscalls.

## Remaining Work

Next milestones should focus on:

- Response encoding without extra value copies.
- Pipelining multiple outstanding requests on one connection.
- Sharded engine ownership for concurrent clients.
