# FishyWebRTC Transport (Modernized Fork)

**FishyWebRTC** is a high-performance WebRTC transport for [FishNet](https://github.com/FirstGearGames/FishNet). It enables **WebGL clients** to connect to dedicated servers (such as those hosted on Edgegap) using WebRTC DataChannels with HTTP/HTTPS signaling.

This repository is a production-hardened fork of [cakeslice/FishyWebRTC](https://github.com/cakeslice/FishyWebRTC), modernized for Unity 6000 and the latest FishNet versions.

## 🚀 Key Improvements in this Fork

- **Production Ready**: Optimized for dedicated server deployments (Edgegap, AWS, etc.) with HTTPS signaling support.
- **Modern Unity Support**: Fully compatible with **Unity 6000.2+** and `com.unity.webrtc` 3.0.0-pre.7.
- **FishNet v4 Compatibility**: Updated to work with FishNet 4.6.19R+ (January 2026), resolving previous logging and API incompatibilities.
- **Zero-Config UPM**: Packaged for Unity Package Manager (UPM) with `versionDefines`, automatically configuring the `ENABLE_WEBRTC` symbol when dependencies are met.
- **Enhanced Configuration**: Added runtime API for configuring HTTPS and signaling ports, essential for dynamic cloud deployments.
- **Production HTTPS Support**: New "Suppress CORS Headers" toggle prevents duplicate CORS headers when running behind a reverse proxy (Edgegap TLS Upgrade), fixing HTTPS WebGL connection failures.

## Prerequisites

- **Unity 6000.2+**
- **FishNet 4.6.19R+** (January 14th, 2026 or newer — installed in Assets)
- `com.unity.webrtc` 3.0.0-pre.7 (auto-installed as dependency)

## Installation

### Option A: Local Package (recommended for development)

1. Copy `com.skillcade.fishy-webrtc/` into your project's `Packages/` folder
2. Add to `Packages/manifest.json`:
   ```json
   "com.skillcade.fishy-webrtc": "file:com.skillcade.fishy-webrtc"
   ```

### Option B: Import .unitypackage

1. Install `com.unity.webrtc` 3.0.0-pre.7 via Package Manager (Add by name: `com.unity.webrtc`)
2. Double-click the `.unitypackage` file → Import All

### Option D: Git URL (recommended for teams)

1. Package Manager → **Add package from git URL**
2. Paste: `https://github.com/KGonzalezASC/FishyWebRTC.git`
3. `com.unity.webrtc` is auto-installed as a dependency

## Setup

`ENABLE_WEBRTC` is **automatically defined** when `com.unity.webrtc` is detected (via asmdef `versionDefines`). No manual scripting defines needed.

1. Add the `FishyWebRTC` component to your NetworkManager GameObject
2. Configure ICE servers (STUN/TURN) in the Inspector
3. Set signaling address and port

### Edgegap Production Configuration

Two settings are required for WebGL builds served over HTTPS (production):

**Inspector (Server component):**
- Enable **Suppress CORS Headers** — Edgegap's TLS proxy already adds CORS headers. If the server also sets them, browsers reject the response. This toggle disables the server-side headers so the proxy owns them.

**Runtime (Client, before `StartConnection()`):**
```csharp
var transport = GetComponent<FishyWebRTC>();
transport.SetHTTPS(true);          // Use HTTPS for signaling
transport.SetNoClientPort(true);   // Port 443 is implied, omit from URL
transport.SetClientAddress("your-server.edgegap.net");
```

> Without **Suppress CORS Headers** on the server, HTTPS WebGL builds will fail at the signaling step even if everything else is correct.

## Docker Testing Scenarios

### Compatibility Matrix

| Scenario | Works? | Notes |
|----------|--------|-------|
| WSL2 + `--network=host` → LAN devices | ✅ | Best for multi-device testing |
| WSL2 + `--network=host` → Same device | ❌ | ICE fails (Windows↔WSL2 NAT) |
| WSL2 + ngrok (inside WSL2) → WAN | ✅ | Use for external/cellular testing |
| Docker Desktop + ngrok → Same device | ⚠️ | Works with proper CORS headers |
| Edgegap Cloud | ✅ | Native Linux, no NAT issues |

### WSL2 with Host Networking (Recommended for LAN)

```bash
# Start Docker in WSL2 with host networking
wsl -d archlinux docker run --rm --network=host <IMAGE>
```

- Server advertises your LAN IP (e.g., `192.168.0.65`)
- LAN devices connect via HTTP: `http://192.168.0.65:7777`
- **Same-device testing fails** due to Windows↔WSL2 routing

> ⚠️ If using `--network=host`, remove any Windows portproxy rules that may conflict:
> ```powershell
> netsh interface portproxy show v4tov4
> netsh interface portproxy delete v4tov4 listenaddress=127.0.0.1 listenport=7777
> ```

### WSL2 + ngrok (For WAN Testing)

```bash
# Run ngrok INSIDE WSL2 (critical — Windows ngrok can't reach WSL2)
wsl -d archlinux /tmp/ngrok http 7777
```

Client settings: **HTTPS** ✅ | **No Client Port** ✅ | Address: `<tunnel>.ngrok-free.dev`

### Same-Device Testing Limitations

WebRTC ICE fails between a Windows browser and WSL2/Docker server due to mDNS resolution and UDP routing across network namespaces. **Workarounds:**
1. Use a second device (phone, laptop) for testing
2. Run server natively on Windows (no Docker)
3. Add a TURN server to relay WebRTC traffic

## Troubleshooting

| Error | Cause | Fix |
|-------|-------|-----|
| `SocketException: Address already in use` | Windows portproxy conflicts | Remove portproxy rule (see above) |
| `CORS preflight did not succeed (502)` | ngrok can't reach server | Run ngrok inside WSL2, not Windows |
| `ICE failed, add a TURN server` | NAT traversal failure | Use different device or add TURN server |
| `NullReferenceException` after server start | Secondary error from socket bind | Check for port conflicts first |

## Edgegap Cloud Deployment

Edgegap is simpler than local Docker because it uses native Linux with a public IP directly on the container — no NAT traversal issues.

### Key Environment Variables

| Variable | Purpose |
|----------|---------|
| `ARBITRIUM_PUBLIC_IP` | Use for ICE candidates |
| `ARBITRIUM_PORT_GAMEPORT_EXTERNAL` | Client connection port (randomized) |
| `ARBITRIUM_PORT_GAMEPORT_INTERNAL` | Server listener port |

## License

Based on [cakeslice/FishyWebRTC](https://github.com/cakeslice/FishyWebRTC) — MIT License.
