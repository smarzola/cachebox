# Quickstart

## Build

```sh
cargo build
```

## Run

```sh
cargo run --bin cachebox -- --bind 127.0.0.1:7400
```

Useful startup options:

```sh
cargo run --bin cachebox -- \
  --bind 127.0.0.1:7400 \
  --max-body-bytes 8388608 \
  --max-memory-bytes 67108864 \
  --max-value-bytes 8388608 \
  --cleanup-interval-ms 250 \
  --cleanup-max-entries-per-tick 128
```

Show all options:

```sh
cargo run --bin cachebox -- --help
```

## Store A Value

```sh
curl --http2-prior-knowledge -i \
  -X PUT 'http://127.0.0.1:7400/v1/namespaces/default/keys/user%3A123' \
  -H 'Cachebox-TTL: 300s' \
  -H 'Cachebox-Tags: user:123,org:9' \
  -H 'Cachebox-Cost: 42' \
  -H 'Content-Type: application/octet-stream' \
  --data-binary 'cached bytes'
```

Expected status:

```text
HTTP/2 201 Created
```

## Read A Value

```sh
curl --http2-prior-knowledge -i \
  'http://127.0.0.1:7400/v1/namespaces/default/keys/user%3A123'
```

Fresh values return `200 OK` and include:

```text
Cachebox-Status: hit
```

## Delete A Value

```sh
curl --http2-prior-knowledge -i \
  -X DELETE 'http://127.0.0.1:7400/v1/namespaces/default/keys/user%3A123'
```

Expected status:

```text
HTTP/2 204 No Content
```

## Check Metrics

```sh
curl --http2-prior-knowledge 'http://127.0.0.1:7400/metrics'
```

## Run Tests

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## Run Benchmarks

```sh
cargo run --bin cachebox-bench
```
