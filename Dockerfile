#
# Stage 1: build the Rust binary.
#
FROM rust:1.94-slim-trixie@sha256:cf09adf8c3ebaba10779e5c23ff7fe4df4cccdab8a91f199b0c142c53fef3e1a AS builder

WORKDIR /build

RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev libclang-dev clang \
 && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin wyoming-supertonic

#
# Stage 2: download the Supertonic-2 assets from Hugging Face.
#
FROM debian:trixie-slim@sha256:4ffb3a1511099754cddc70eb1b12e50ffdb67619aa0ab6c13fcd800a78ef7c7a AS assets

RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY scripts/download-assets.sh /usr/local/bin/download-assets.sh
ENV ASSETS_DIR=/assets
RUN sh /usr/local/bin/download-assets.sh

#
# Stage 3: runtime (distroless — ~20 MB base, glibc, non-root).
#
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:8f960b7fc6a5d6e28bb07f982655925d6206678bd9a6cde2ad00ddb5e2077d78

WORKDIR /app
COPY --from=builder /build/target/release/wyoming-supertonic /usr/local/bin/wyoming-supertonic
COPY --from=assets /assets /app/assets

ENV SUPERTONIC_ONNX_DIR=/app/assets/onnx \
    SUPERTONIC_VOICES_DIR=/app/assets/voice_styles \
    SUPERTONIC_DEFAULT_VOICE=F2 \
    PORT=10220 \
    TOTAL_STEPS=3 \
    MODEL_SPEED=1.0 \
    TRANSFORM_SPEED=1.4

EXPOSE 10220
ENTRYPOINT ["/usr/local/bin/wyoming-supertonic"]
