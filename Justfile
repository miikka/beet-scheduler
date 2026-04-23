# SPDX-FileCopyrightText: 2026 Miikka Koskinen
#
# SPDX-License-Identifier: MIT

run:
    cargo run

test:
    cargo llvm-cov nextest

test-docker:
    ./tests/docker_test.sh

format:
    cargo fmt

check: ci

lint:
    cargo fmt --check
    cargo clippy

ci: lint check-cov test-docker

check-cov:
    cargo llvm-cov nextest --json | python3 scripts/check_coverage.py

# Update the coverage baseline data for the current platform
cov-baseline:
    cargo llvm-cov nextest --json | python3 scripts/check_coverage.py --save-baseline

docker-build:
    docker build -t beet-scheduler .

docker-run: docker-build
    docker run --rm --init -p 3000:3000 beet-scheduler
