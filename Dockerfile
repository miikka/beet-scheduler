# SPDX-FileCopyrightText: 2026 Miikka Koskinen
#
# SPDX-License-Identifier: MIT

FROM rust:1-bookworm AS builder

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --release

FROM debian:bookworm-slim

LABEL org.opencontainers.image.source=https://github.com/miikka/beet-scheduler

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/beet-scheduler .
COPY templates/ templates/
COPY static/ static/

RUN useradd -r -u 1001 bs
RUN mkdir /data && chown 1001 /data

USER 1001

EXPOSE 3000

ENV DATABASE_PATH=/data/beet-scheduler.db

# Mount your data directory at runtime:
#   -v /path/to/data:/data
VOLUME /data

ENTRYPOINT ["tini", "--"]
CMD ["./beet-scheduler"]
