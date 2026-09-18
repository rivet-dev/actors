# Rivetkit-rust fix: `$Uint8Array` encoding parity with TS

A narrow framework fix to bring rivetkit-rust to parity with rivetkit-typescript on **`Uint8Array` byte payloads only**. Precondition for the agent-os integration (PLAN2.md).

**Explicitly scope-limited:** the only TS convention this implements is `JSON_COMPAT_UINT8_ARRAY`. Other JSON-compat types (`$BigInt`, `$ArrayBuffer`, `$Undefined`, `$Set`, `Date`, `RegExp`, `Error`, `Map`, etc.) are **not** in scope. They can be added when a real consumer needs them. agent-os only returns `Uint8Array`-shaped bytes (`readFile`, `vmFetch.body`, batch-read `content`); nothing else.

**TS is the source of truth.** TS sits on at least one end of every action call. The wire convention `["$Uint8Array", base64]` is what TS emits and what TS expects. The Rust framework mirrors it on both encode and decode sides.

---

## The gap

TS rivetkit handles byte payloads transparently:

```ts
readFile: async (c, path) => agentOs.readFile(path),  // returns Uint8Array
```

The user returns a `Uint8Array`, the framework wraps as `["$Uint8Array", base64]`, the receiving client revives. Works across bare/cbor/json.

Rust rivetkit has no equivalent:

```rust
action.ok(&bytes);  // Vec<u8>
```

Bytes round-trip cleanly on bare. On cbor and json they get mangled into a number array because the engine decodes through `serde_json::Value` (no byte variant) at `rivetkit-core/src/registry/inspector.rs::decode_cbor_json_or_null`.

This is a framework-feature-parity miss. Every Rust-defined actor that returns bytes is silently broken on non-bare encodings.

---

## TS reference

`rivetkit-typescript/packages/rivetkit/src/common/encoding.ts:14`:

```ts
const JSON_COMPAT_UINT8_ARRAY = "$Uint8Array";  // capital U
```

Encode (`encodeJsonCompatValue`):
```ts
if (input instanceof Uint8Array) {
    return [JSON_COMPAT_UINT8_ARRAY, base64EncodeUint8Array(input)];
}
```

Decode (`reviveJsonCompatValue`):
```ts
if (input[0] === JSON_COMPAT_UINT8_ARRAY) {
    return base64DecodeToUint8Array(input[1]);
}
```

Applied recursively to nested byte fields.

---

## Proposed fix

Three parts. Encode + decode are essential for full parity; the engine cleanup is a follow-up.

### Part 1 — Encode side: auto-wrap in `Action::ok`

**Goal:** mirror TS's `Uint8Array` wrapping. When a user-returned value contains byte payloads, wrap them as `["$Uint8Array", base64]` before CBOR-encoding. Recurse into nested fields.

```rust
// rivetkit-rust/packages/rivetkit/src/encoding.rs (new)
const JSON_COMPAT_UINT8_ARRAY: &str = "$Uint8Array";

pub(crate) fn encode_json_compat<T, W>(value: &T, writer: &mut W) -> anyhow::Result<()>
where
    T: Serialize,
    W: std::io::Write,
{
    let mut adapter = JsonCompatAdapter::new(writer);
    value.serialize(&mut adapter)?;
    Ok(())
}
```

The adapter intercepts `serialize_bytes` calls (`serde_bytes::ByteBuf`, `serde_bytes::Bytes`, `&[u8]` via `serde_bytes`) and emits the 2-element array shape. Plain `Vec<u8>` keeps default behavior (CBOR array) — users opting into `Uint8Array` semantics annotate `#[serde(with = "serde_bytes")]`. Matches TS's explicit `Uint8Array` vs other-typed-array distinction.

All other serde calls pass through to ciborium unchanged. No `$BigInt`, `$ArrayBuffer`, `$Set`, `$Undefined` handling. Other types encode as ciborium's default — same as today.

Swap `Action::ok` (and `WfHistory::reply`, `WfReplay::reply`) to use `encode_json_compat` instead of raw `ciborium::into_writer`. State serialization (`SerializeState::save`) and queue payloads stay raw — consumed by Rust, not JS.

### Part 2 — Decode side: auto-unwrap in `rivetkit-client`

**Goal:** mirror TS's `Uint8Array` revival. When a Rust client receives an action response containing `["$Uint8Array", base64]`, hand the caller bytes instead of the literal array.

Where: `rivetkit-rust/packages/client/`.

```rust
// rivetkit-rust/packages/client/src/encoding.rs (new)
pub(crate) fn revive_json_compat(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) if is_uint8_array_tag(&items) => {
            // ["$Uint8Array", base64] → bytes (whatever shape this crate uses)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items.into_iter().map(revive_json_compat).collect(),
        ),
        serde_json::Value::Object(_) => /* recurse */ value,
        other => other,
    }
}

fn is_uint8_array_tag(items: &[serde_json::Value]) -> bool {
    items.len() == 2
        && items[0].as_str() == Some("$Uint8Array")
        && items[1].is_string()
}
```

Other tagged arrays (`["$BigInt", ...]`, `["$Set", ...]`) and non-tagged arrays pass through unchanged.

Hook into the action-response decode site. Confirm `rivetkit-client`'s response shape during implementation.

### Part 3 — Engine routing cleanup (follow-up)

**Goal:** `rivetkit-core` shouldn't lossy-decode action responses through `serde_json::Value` in the first place.

Audit callers of `decode_cbor_json_or_null` (`rivetkit-core/src/registry/inspector.rs:598-609`). Split:
- Inspector display path: keep `serde_json::Value` intermediate (browser tab shows bytes as base64 or hex).
- Action-response forward path: sibling that forwards encoded bytes opaquely.

Can land any time. Parts 1+2's wrapping convention survives the lossy decode anyway — `["$Uint8Array", base64]` is JSON-native.

---

## Tests

### Part 1 — Rust encode (`rivetkit-rust/packages/rivetkit/tests/encoding.rs`)

```rust
#[test]
fn byte_buf_wraps_as_json_compat_uint8_array() { ... }

#[test]
fn nested_byte_field_in_struct_wraps() {
    #[derive(Serialize)]
    struct Reply { status: u16, body: serde_bytes::ByteBuf }
    // assert intermediate["body"][0] == "$Uint8Array"
    // assert intermediate["body"][1] == base64
}

#[test]
fn plain_vec_u8_stays_as_array() { ... }

#[test]
fn non_byte_types_pass_through_unchanged() { ... }
```

### Part 2 — Rust decode (`rivetkit-rust/packages/client/tests/encoding.rs`)

```rust
#[test]
fn json_compat_uint8_array_revives_to_bytes() { ... }

#[test]
fn nested_byte_field_revives_inside_struct() { ... }

#[test]
fn non_byte_arrays_pass_through() { ... }

#[test]
fn unrelated_tagged_arrays_pass_through() { ... }
```

### Round-trip parity

```rust
#[test]
fn encode_then_decode_round_trips_bytes() {
    let original = b"round-trip data".to_vec();
    let value = serde_bytes::ByteBuf::from(original.clone());
    let encoded = encode_json_compat_to_vec(&value).unwrap();
    let intermediate: serde_json::Value =
        ciborium::from_reader(&encoded[..]).unwrap();
    let revived = revive_json_compat(intermediate);
    assert_eq!(revived_as_bytes(&revived).unwrap(), original);
}
```

### TS parity (cross-language)

A Rust test in `rivetkit-rust/packages/rivetkit/tests/encoding_fixtures.rs` writes Rust-encoded output to a fixture file. A Vitest test in `rivetkit-typescript/packages/rivetkit/tests/byte-encoding-parity.test.ts` reads the fixture and asserts:
- TS `encodeJsonCompatValue` produces the same shape on the same input.
- TS `reviveJsonCompatValue` revives Rust-encoded bytes correctly.

Keeps both sides honest.

### End-to-end (after Parts 1+2 land)

The agent-os driver suite cell `writeFile and readFile round-trip` should pass for cbor and json without any agent-os-specific changes. That's the all-the-way-through validation.

---

## Scope

### Part 1 — Encode (rivetkit-rust)

- `rivetkit-rust/packages/rivetkit/src/encoding.rs` (new, ~150 lines).
- `rivetkit-rust/packages/rivetkit/src/event.rs` (~5 line change in `Action::ok`, `WfHistory::reply`, `WfReplay::reply`).
- `rivetkit-rust/packages/rivetkit/src/lib.rs` (add `pub mod encoding`).
- `rivetkit-rust/packages/rivetkit/tests/encoding.rs` (new).

### Part 2 — Decode (rivetkit-client)

- `rivetkit-rust/packages/client/src/encoding.rs` (new, ~120 lines).
- The action-response decode site (find via grep).
- `rivetkit-rust/packages/client/tests/encoding.rs` (new).

### Part 3 — Engine cleanup (rivetkit-core, follow-up)

- `rivetkit-core/src/registry/inspector.rs` (split function), plus every caller.

---

## Ordering

1. Land Parts 1 + 2 in `rivetkit-rust`.
2. Verify via the focused Rust unit tests, the round-trip test, and the cross-language TS-parity test.
3. Then proceed with PLAN2's Phase 1 (agent-os bones).
4. Part 3 (engine routing cleanup) can land any time, before or after agent-os ships.
