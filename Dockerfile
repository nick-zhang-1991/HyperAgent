# HyperAgent Docker Image
# Build: docker build -t hyperagent .
# Run:   docker run -it hyperagent hyper run "explain this code"

FROM rust:1.82-slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release && strip target/release/hyperagent

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates git curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hyperagent /usr/local/bin/hyperagent

ENV HYPER_LANG=en
ENTRYPOINT ["hyperagent"]
CMD ["--help"]
