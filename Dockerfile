FROM rust:1.86-slim AS builder

RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/fcjp /app/fcjp
RUN chmod +x /app/fcjp

VOLUME ["/data"]
WORKDIR /data
ENTRYPOINT ["/app/fcjp"]
CMD ["--help"]
