#!/bin/bash
set -e

echo "=== Building ==="
cargo build --release 2>&1

echo ""
echo "=== Running unit tests ==="
cargo test 2>&1

echo ""
echo "=== Testing basic functionality ==="
echo '你好' | timeout 30 ./target/release/agent || true
echo '---'

echo ""
echo "=== Testing /help command ==="
echo '/help' | timeout 10 ./target/release/agent || true
echo '---'

echo ""
echo "=== Testing /skills command ==="
echo '/skills' | timeout 10 ./target/release/agent || true
echo '---'

echo ""
echo "=== Testing /model command ==="
echo '/model' | timeout 10 ./target/release/agent || true
echo '---'

echo ""
echo "=== All tests completed ==="
