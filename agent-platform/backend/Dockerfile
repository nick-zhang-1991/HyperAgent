FROM rust:1.82-slim as builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* .
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release 2>/dev/null; rm -rf src
COPY src/ src/
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/agent-platform /usr/local/bin/
EXPOSE 4000
CMD ["agent-platform"]
