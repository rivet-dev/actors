#!/usr/bin/env bash
# Commands from the guide, in order.
set -euo pipefail

docker run -d \
  --name rivet-engine \
  -p 6420:6420 \
  -v rivet-data:/data \
  -e RIVET__FILE_SYSTEM__PATH="/data" \
  rivetdev/engine:latest

curl http://localhost:6420/health
