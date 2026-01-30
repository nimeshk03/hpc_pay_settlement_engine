# Multi-stage Dockerfile for Settlement Engine
# Stage 1: Build application
FROM rust:1.83-slim AS builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code, migrations, and benches
COPY src ./src
COPY migrations ./migrations
COPY benches ./benches

# Build application (use --locked to respect Cargo.lock)
RUN cargo build --release --locked --bin settlement_engine

# Stage 2: Runtime image
FROM debian:bookworm-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r settlement && useradd -r -g settlement settlement

# Copy binary from builder
COPY --from=builder /app/target/release/settlement_engine /usr/local/bin/settlement_engine

# Copy migrations
COPY --from=builder /app/migrations /app/migrations

# Create necessary directories
RUN mkdir -p /app/config /app/logs && \
    chown -R settlement:settlement /app

# Switch to non-root user
USER settlement

# Expose application port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/settlement_engine", "--health-check"] || exit 1

# Set environment variables
ENV RUST_LOG=info
ENV DATABASE_URL=postgresql://postgres:postgres@postgres:5432/settlement_engine
ENV REDIS_URL=redis://redis:6379
ENV KAFKA_BROKERS=kafka:9092

# Run the application
CMD ["/usr/local/bin/settlement_engine"]
