FROM rust:1.95.0-slim-bookworm AS builder

RUN apt-get update -y \
  && apt-get install -y --no-install-recommends build-essential pkg-config ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src ./src

ARG RUSTORY_BUILD_REVISION
ARG RUSTORY_BUILD_REVISION_SOURCE=git
ARG RUSTORY_BUILD_DIRTY=false

ENV RUSTORY_BUILD_REVISION=${RUSTORY_BUILD_REVISION}
ENV RUSTORY_BUILD_REVISION_SOURCE=${RUSTORY_BUILD_REVISION_SOURCE}
ENV RUSTORY_BUILD_DIRTY=${RUSTORY_BUILD_DIRTY}

RUN cargo build --release --locked --bin rr

FROM debian:bookworm-slim

COPY --from=builder /app/target/release/rr /usr/local/bin/rr

USER 65532:65534
ENTRYPOINT ["/usr/local/bin/rr"]
CMD ["tracker-serve", "--bind", "0.0.0.0:8850"]
