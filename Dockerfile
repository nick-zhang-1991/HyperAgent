# HyperAgent — Docker image
# Build: docker build -t hyperagent .
# Run:   docker run -it --rm \
#          -v "$(pwd):/workspace" \
#          -e HYPER_LLM_API_KEY="sk-..." \
#          hyperagent run "task"

FROM rust:1.78-slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY .cargo/ ./.cargo/

RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/hyperagent /usr/local/bin/hyper

WORKDIR /workspace
ENTRYPOINT ["hyper"]
CMD ["--help"]
