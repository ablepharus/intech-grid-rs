#!/usr/bin/env bash
cargo test 2>&1 | grep -E '^(error|warning)' -A 8 | head -30
echo '--- results ---'
cargo test 2>&1 | grep -E '^test result|FAILED'
echo "clippy: $(cargo clippy --all-targets 2>&1 | grep -cE '^(warning|error)')"
echo "doc:    $(cargo doc --no-deps 2>&1 | grep -cE '^(warning|error)')"
cargo build --release 2>&1 | tail -1
