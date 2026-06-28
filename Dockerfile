FROM rust:1-trixie AS builder

WORKDIR /app

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --locked --release --bin cachebox

FROM gcr.io/distroless/cc-debian13:nonroot AS runtime

COPY --from=builder /app/target/release/cachebox /usr/local/bin/cachebox

EXPOSE 7400 7401

ENTRYPOINT ["/usr/local/bin/cachebox"]
CMD ["--bind", "0.0.0.0:7400", "--native-bind", "0.0.0.0:7401"]
