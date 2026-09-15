# Dead-workflow index backfill

The `gasoline_dead_wf_backfill` workflow rebuilds the dead-workflow index for
existing workflow records. Each activity processes at most 1,000 workflows and
uses the existing three-second early transaction deadline. The activity's
`last_key: Option<Vec<u8>>` and continuation output remain unchanged.

RocksDB materializes a range before exposing it through the UniversalDB stream
interface. A consumer-side workflow count does not bound that allocation. The
backfill therefore discovers one workflow with a key-only seek,
reads its name, error, and status markers directly, and advances to the end of
that workflow's data prefix. Output detection uses a separate key-only seek so
any output chunk excludes the workflow without loading its output payload.
Classification point reads are serializable, and an explicit read conflict
covers the output subtree to detect concurrent output insertion.
Input and state chunks are never traversed for classification.

The returned cursor is the first unprocessed range boundary. Cursors written by
the original implementation, which pointed at an actual first workflow key,
remain valid. A cursor advances only after the workflow's complete classification
and optional index write. The index writes and returned cursor share the same
transaction outcome. An interrupted activity can safely repeat an already
committed chunk because index keys are deterministic. A timeout during reads
leaves that workflow as the next unprocessed range.

The regression test uses the actual RocksDB-backed `DatabaseDebug` operation.
It seeds a workflow with four MiB of irrelevant input, verifies less than
64 KiB is read to classify it, checks all exclusion markers, and exercises
legacy cursors, one-workflow chunks, and replayed input. Run:

```sh
RIVET_TEST_DATABASE=filesystem RIVET_TEST_PUBSUB=memory cargo test -p gasoline --lib backfill_skips_large_payloads --locked
```

This bounds the backfill's reads, not every engine allocation. It does not
change the RocksDB driver, actor data format, workflow input serialization,
or Envoy protocol.
