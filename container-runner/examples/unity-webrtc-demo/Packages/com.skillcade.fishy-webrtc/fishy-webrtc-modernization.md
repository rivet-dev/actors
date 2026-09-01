---
description: Modernizing FishyWebRTC transport for improved FishNet compatibility
---

# FishyWebRTC Modernization

Quick fixes to bring the 3-year-old FishyWebRTC transport up to modern FishNet standards.

## Prerequisites
- FishyWebRTC already integrated (Phase 1-2 of FishyWebRTC Integration complete)
- Located at: `Assets/FishNet/Runtime/Transporting/Transports/FishyWebRTC/`

---

## Changes

### 1. Fix MTU Header Accounting (5 min)

**File**: `FishyWebRTC.cs`

**Find** (around line 270):
```csharp
public override int GetMTU(byte channel)
{
    return _mtu;
}
```

**Replace with**:
```csharp
/// <summary>
/// WebRTC DataChannel has ~24 bytes overhead (DTLS + SCTP headers).
/// Subtract this from MTU to prevent fragmentation.
/// </summary>
private const int WEBRTC_DATACHANNEL_OVERHEAD = 24;

public override int GetMTU(byte channel)
{
    return _mtu - WEBRTC_DATACHANNEL_OVERHEAD;
}
```

---

### 2. Add Configurable Timeout (10 min)

**File**: `FishyWebRTC.cs`

**Add field** in `#region Serialized` (around line 30):
```csharp
[Header("Timeouts")]
/// <summary>
/// Seconds before connection times out. -1 to disable.
/// </summary>
[Tooltip("Seconds before connection times out. -1 to disable.")]
[SerializeField]
private float _timeout = 30f;
```

**Find** (around line 145):
```csharp
public override float GetTimeout(bool asServer)
{
    return -1f;
}
```

**Replace with**:
```csharp
public override float GetTimeout(bool asServer)
{
    return _timeout;
}
```

---

### 3. (Optional) Add Unreliable MTU Setting (5 min)

For fighting games, you may want a smaller unreliable MTU to reduce latency.

**Add field** after the existing `_mtu` field:
```csharp
[Tooltip("Maximum transmission unit for the unreliable channel. Smaller = lower latency.")]
[Range(MINIMUM_MTU, MAXIMUM_MTU)]
[SerializeField]
private int _unreliableMtu = 512;
```

**Update GetMTU**:
```csharp
public override int GetMTU(byte channel)
{
    int baseMtu = (channel == (byte)Channel.Unreliable) ? _unreliableMtu : _mtu;
    return baseMtu - WEBRTC_DATACHANNEL_OVERHEAD;
}
```

---

### 4. (Advanced) Batching in IterateOutgoing (2-4 hrs)

Batching combines multiple small packets into fewer larger ones, reducing syscall overhead.

**GGPO Considerations:**

| Scenario | Batching Impact | Recommendation |
|----------|----------------|----------------|
| **60 FPS game loop, 1 input packet/frame** | Negligible benefit - already 1 packet | ❌ Skip |
| **Multiple RPC calls per frame** | Reduces packet count | ⚠️ Consider |
| **High player count (4+ players)** | Reduces N packets to 1 | ✅ Helps |

**Why batching can HURT GGPO:**

GGPO relies on **immediate packet delivery** for accurate frame timing. Batching introduces:
1. **Buffer delay** - packets wait to be batched
2. **Jitter** - inconsistent delivery timing
3. **Input lag** - if inputs are held for batching

**Safe batching approach** (only batch within same frame):

```csharp
// In IterateOutgoing - only send batched packets at end of frame
// Do NOT hold packets across frames
public override void IterateOutgoing(bool server)
{
    // Flush all queued packets immediately - no cross-frame batching
    // This is safe for GGPO as packets still go out same frame
    
#if !UNITY_WEBGL || UNITY_EDITOR
    if (server)
        _server.IterateOutgoing();
    else
#endif
        _client.IterateOutgoing();
}
```

**Verdict for your fighting game:**
- Your GGPO implementation sends inputs via FishNet RPCs
- FishNet already batches RPCs at the transport level
- **Skip transport-level batching** - FishNet handles it
- Focus on the MTU/timeout fixes instead

---

## Verification

// turbo
1. Build WebGL and verify no compile errors
2. Check Unity Console for any MTU warnings during connection
3. Test packet delivery still works (GGPO sync message flow)

---

## Notes

- These changes are backward-compatible
- No changes needed to server or client socket classes
- If you encounter issues, revert to `return _mtu;` in GetMTU
