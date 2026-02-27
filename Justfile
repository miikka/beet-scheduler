test:
    cargo llvm-cov nextest

format:
    cargo fmt

check: ci

ci:
    cargo fmt --check
    cargo clippy
    cargo llvm-cov nextest --json | python3 scripts/check_coverage.py

# Update the coverage baseline data for the current platform
cov-baseline:
    cargo llvm-cov nextest --json | python3 scripts/check_coverage.py --save-baseline
