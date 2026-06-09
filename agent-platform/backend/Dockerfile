# Multi-stage Docker build for agent-platform backend
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* .
RUN mkdir src && echo "fn main(){}" > src/main.rs
RUN cargo build --release 2>/dev/null; rm -rf src
COPY src/ src/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/agent-platform /usr/local/bin/
ENV JWT_SECRET=change-me-in-production
EXPOSE 4000
CMD ["agent-platform"]
