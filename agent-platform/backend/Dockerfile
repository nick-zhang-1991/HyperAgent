FROM rust:1.82-slim
WORKDIR /app
COPY . .
RUN cargo build --release
EXPOSE 4000
ENV JWT_SECRET=agent-platform-v3-secret
CMD ["./target/release/agent-platform"]
