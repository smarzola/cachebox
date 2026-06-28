# Performance Uplift Response Copy

Milestone 2 removes the intermediate owned response value for the native cached
get server path.

## Change

- Added `Engine::get_ref`, which exposes hit and stale values to a callback as
  borrowed slices while preserving the existing owned `Engine::get` behavior.
- Added a borrowed response payload encoder for hit, stale, and miss responses.
- Updated the server's borrowed `Get` fast path to encode the response frame
  directly from the engine value into the per-connection response buffer.

The public client API still returns owned response bytes. This change removes a
server-side clone for native `Get`; it does not change protocol bytes.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 23166 | 23500 | +1.4% |
| `concurrent_get_16` | `loopback_tcp` | 100458 | 100583 | +0.1% |
| `single_key_get` | `loopback_unix` | 14083 | 13834 | -1.8% |
| `concurrent_get_16` | `loopback_unix` | 70000 | 71500 | +2.1% |

The result is mixed and close to local benchmark noise for tiny values. Removing
the server-side value clone is still structurally useful, but it is not the
dominant latency cost for the current hot cached-get benchmark. Socket
round-trips, frame parsing, atomics, and the global engine mutex remain more
important for these small payloads.

## Remaining Copies

- Batch get still builds owned `GetOutcome` values for each key.
- Lease hit and stale responses still clone value bytes.
- The public client still decodes hit and stale values into owned `Vec<u8>`.

Those copies should be revisited with larger value-size benchmarks before
adding more complexity.
