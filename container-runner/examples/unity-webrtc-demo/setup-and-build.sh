#!/usr/bin/env bash
# Resolve packages, wire the FishNet starter scene to FishyWebRTC, and build the
# dedicated server or native macOS client. Requires an activated Unity license.
#
# Usage: unity-webrtc-demo/setup-and-build.sh [demo|mac|linux]   (default: demo)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TARGET="${1:-demo}"
UNITY="${UNITY:-/Applications/Unity/Hub/Editor/6000.5.2f1/Unity.app/Contents/MacOS/Unity}"

[ -x "$UNITY" ] || { echo "Unity editor not found at $UNITY (set \$UNITY)"; exit 1; }

run_unity() { # $1 = description; rest = args
  local desc="$1"; shift
  echo "==> Unity: $desc"
  "$UNITY" -batchmode -quit -nographics -projectPath "$HERE" -logFile - "$@" 2>&1 \
    | grep -iE 'error|exception|fail|build|bootstrap|Packages|WebRTC|FishNet|Succeeded' | tail -40
}

# 1) First open resolves embedded FishNet and FishyWebRTC packages.
run_unity "resolve packages" 

# 2) Create/update the starter scene and wire it to FishyWebRTC.
run_unity "wire starter scene to FishyWebRTC" -executeMethod ProjectBootstrap.Setup

# 3) Build the dedicated server.
case "$TARGET" in
  demo)  run_unity "build macOS demo"    -executeMethod BuildScript.BuildDemoMac ;;
  mac)   run_unity "build macOS server"  -executeMethod BuildScript.BuildServerMac ;;
  linux) run_unity "build Linux server"  -executeMethod BuildScript.BuildServerLinux ;;
  *) echo "unknown target $TARGET (use demo|mac|linux)"; exit 1 ;;
esac

echo "==> Done. Build under container-runner/examples/unity-webrtc-demo/Builds/"
find "$HERE/Builds" -maxdepth 3 -type f -name 'GameServer*' 2>/dev/null
