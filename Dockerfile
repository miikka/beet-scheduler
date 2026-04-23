# SPDX-FileCopyrightText: 2026 Miikka Koskinen
# SPDX-License-Identifier: MIT

# Customization point: set the name of the binary built by Cargo.
ARG BINARY_NAME=beet-scheduler

FROM rust:1-trixie AS chef
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    cargo install --locked cargo-chef
WORKDIR /build

FROM chef AS planner
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG BINARY_NAME
RUN mkdir -p /empty
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/build/target \
     cargo chef cook --release --recipe-path recipe.json
COPY . .
# We have to copy the binary out of `target` as the last step because it is a cache mount.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin "$BINARY_NAME" && \
    cp "target/release/$BINARY_NAME" app

################################################################################

FROM gcr.io/distroless/cc-debian13:nonroot

LABEL org.opencontainers.image.source=https://github.com/miikka/beet-scheduler

WORKDIR /app
COPY --from=builder /build/app /app/app

COPY --from=builder --chown=65532:65532 /empty /data

COPY static/ static/
COPY templates/ templates/

ENV DATABASE_PATH=/data/beet-scheduler.db
EXPOSE 3000

# Mount your data directory at runtime:
#   -v /path/to/data:/data
VOLUME /data

CMD ["/app/app"]
