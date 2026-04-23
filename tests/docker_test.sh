#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Miikka Koskinen
# SPDX-License-Identifier: MIT

# Test the Docker build: build the image, start a container, and run basic checks.

set -euo pipefail

IMAGE="beet-scheduler:test"
CONTAINER="beet-scheduler-test"
PORT=3099

cleanup() {
    echo "Cleaning up..."
    docker rm -f "$CONTAINER" 2>/dev/null || true
}
trap cleanup EXIT

echo "==> Building Docker image..."
docker build -t "$IMAGE" .

echo "==> Starting container on port $PORT..."
docker run -d --name "$CONTAINER" --init -p "$PORT:3000" "$IMAGE"

echo "==> Waiting for server to be ready..."
for i in $(seq 1 30); do
    if curl -sf "http://localhost:$PORT/" > /dev/null 2>&1; then
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "FAIL: server did not become ready"
        docker logs "$CONTAINER"
        exit 1
    fi
    sleep 1
done

echo "==> Checking home page..."
status=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$PORT/")
if [ "$status" != "200" ]; then
    echo "FAIL: expected 200 from /, got $status"
    exit 1
fi

echo "==> Checking that home page contains expected content..."
body=$(curl -s "http://localhost:$PORT/")
if ! echo "$body" | grep -qi "beet"; then
    echo "FAIL: home page does not contain expected content"
    exit 1
fi

echo "==> Creating a meeting..."
response=$(curl -s -o /dev/null -w '%{http_code}:%{redirect_url}' \
    -X POST "http://localhost:$PORT/meetings" \
    -d 'title=Test+Meeting&slot_date%5B%5D=2026-05-01&slot_time%5B%5D=09%3A00')
code="${response%%:*}"
if [ "$code" != "303" ]; then
    echo "FAIL: expected 303 from POST /meetings, got $code"
    exit 1
fi

echo "==> Checking meeting page..."
redirect_url="${response#*:}"
# Extract the path from the redirect URL
meeting_path="${redirect_url#http://localhost:$PORT}"
meeting_status=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$PORT$meeting_path")
if [ "$meeting_status" != "200" ]; then
    echo "FAIL: expected 200 from $meeting_path, got $meeting_status"
    exit 1
fi

echo "==> Checking static assets..."
css_status=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$PORT/static/style.css")
if [ "$css_status" != "200" ]; then
    echo "FAIL: expected 200 from /static/style.css, got $css_status"
    exit 1
fi

echo "==> All checks passed!"
