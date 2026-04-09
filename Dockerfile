# ============================================================
# Stage 1: Build
# ============================================================
FROM rust:1.91.0-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs && cargo build --release && rm -rf src

# Build real app
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ============================================================
# Stage 2: Runtime
# ============================================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/teslamate-geocoder /usr/local/bin/

EXPOSE 9090

ENTRYPOINT ["teslamate-geocoder"]
CMD ["--batch-size", "10", "--qps", "3", "--metrics"]
