#!/usr/bin/env bash
# Commands from the guide, in order.
set -euo pipefail

docker run -p 6420:6420 rivetdev/engine

git clone https://github.com/rivet-dev/rivet.git
cd rivet
cargo build --release -p rivet-engine
./target/release/rivet-engine
