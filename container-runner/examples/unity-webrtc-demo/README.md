# unity-webrtc-demo

This is the existing FishNet Unity demo copied to use FishyWebRTC instead of the
Bayou WebSocket transport.

- The Linux dedicated server listens on the `PORT` injected by
  `rivet-container-runner`.
- FishyWebRTC serves `GET /offer/` and `POST /answer/` on that port.
- Container Runner v2.3.3 forwards the gateway's `/request/` path to the child, so
  the native Unity client uses
  `https://api.rivet.dev/gateway/<actor_id>@<token>/request/` as its signaling base URL.
- Game traffic uses FishyWebRTC's reliable and unreliable WebRTC data channels.
- The Docker image downloads the stable Container Runner `v2.3.3` release.

## Contents

```text
Assets/                         Demo scene, player, bootstrap, and build scripts
Packages/com.firstgeargames.*   Embedded FishNet 4.7.2
Packages/com.skillcade.*        Embedded FishyWebRTC 1.0.2
Dockerfile                      Unity server plus Container Runner v2.3.3
setup-and-build.sh              Server and native client build helper
```

The embedded FishyWebRTC package is from
[`Skillcade/FishyWebRTC`](https://github.com/Skillcade/FishyWebRTC) at commit
`1cd5c899da0ebe1804706e52fd8aa1e76436a483`. This copy adds a configurable
signaling base path so `offer/` and `answer/` work below Rivet's actor gateway
route. It also adds a Unity.WebRTC native client for the same reliable and
unreliable data channels used by the server.

## Build

Install Unity `6000.5.2f1` with Linux Dedicated Server build support, then run:

```bash
container-runner/examples/unity-webrtc-demo/setup-and-build.sh demo
container-runner/examples/unity-webrtc-demo/setup-and-build.sh linux
```

Build the server image from the Rivet repository root:

```bash
docker build --platform linux/amd64 \
  -f container-runner/examples/unity-webrtc-demo/Dockerfile \
  --build-arg RIVET_RUNNER_VERSION="$(date +%s)" \
  -t unity-webrtc-demo:amd64 .
```

## Connect the native Unity client

Create the actor and obtain its gateway token. Start the native build with the
gateway URL passed to `-url`:

```bash
container-runner/examples/unity-webrtc-demo/Builds/DemoMac/GameDemo.app/Contents/MacOS/Unity\ WebRTC\ Demo \
  -client \
  -url "https://api.rivet.dev/gateway/<actor_id>@<token>/request/" \
  -logFile -
```

The signaling URL must end in `/`:

```text
https://api.rivet.dev/gateway/<actor_id>@<token>/request/
```

FishyWebRTC appends `offer/` and `answer/` to that base path.

## ICE networking

The default configuration is STUN-only and uses both Google's and Cloudflare's
public STUN services:

```text
stun:stun.l.google.com:19302
stun:stun.cloudflare.com:3478
```

No TURN URL or credentials are required for the native P2P path. The client logs
the nominated ICE candidate pair after connecting. A successful direct test
contains both of these lines:

```text
[webrtc] SELECTED ICE PAIR: ... => DIRECT P2P (NO TURN)
[webrtc] ICE PROOF PASSED: native UDP P2P with no TURN relay
```

`WEBRTC_ICE_URL` and `-ice-url` can override the defaults with a comma-separated
list. Rivet forwards HTTP signaling to the container, while the native peers
negotiate the UDP data path directly using ICE.

## Update Container Runner

The Docker build pins `rivet-container-runner` to `v2.3.3`, the version verified
against the current Rivet Cloud engine. Update the URL only after a newer runner
has passed the same local and Cloud actor tests. Keep this example rebased on
the latest repository `main` for the Unity project and deployment configuration.
