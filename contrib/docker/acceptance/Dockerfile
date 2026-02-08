FROM rust:1.89.0-slim-bookworm AS builder

RUN apt-get update -y \
  && apt-get install -y --no-install-recommends build-essential pkg-config ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 캐시 힌트(의존성 레이어)
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src

RUN cargo build --release --locked --bin rr

FROM debian:bookworm-slim

RUN apt-get update -y \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/rr /usr/local/bin/rr

CMD ["rr"]

