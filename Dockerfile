FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin six-disc-changer

# We do not need the Rust toolchain to run the binary!
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install ca-certificates for SSL/TLS connections
RUN apt-get update && apt-get install -y ca-certificates
# Remove apt-cache to save space
RUN apt-get clean && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/six-disc-changer /usr/local/bin
COPY --from=builder /app/templates /app/templates
RUN mkdir data
ENTRYPOINT ["/usr/local/bin/six-disc-changer"]
