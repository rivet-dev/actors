mod common;

use anyhow::Result;
use depot::types::DirtyPage;

fn page(pgno: u32, fill: u8) -> DirtyPage {
	DirtyPage {
		pgno,
		bytes: vec![fill; depot::keys::PAGE_SIZE as usize],
	}
}

#[tokio::test]
async fn history_snapshot_reports_exact_row_sets() -> Result<()> {
	common::test_matrix("depot-history-snapshot", |_tier, ctx| {
		Box::pin(async move {
			// Three commits: txid 1 writes pages 1-2, txid 2 overwrites page 2 and
			// writes page 3, txid 3 writes a page in the second shard.
			ctx.db
				.commit(vec![page(1, 0x11), page(2, 0x12)], 3, 1_000)
				.await?;
			ctx.db
				.commit(vec![page(2, 0x22), page(3, 0x23)], 3, 2_000)
				.await?;
			let far_pgno = depot::keys::SHARD_SIZE + 1;
			ctx.db
				.commit(vec![page(far_pgno, 0x33)], far_pgno, 3_000)
				.await?;

			let branch_id =
				common::database_branch_id(&ctx.udb, ctx.bucket_id, &ctx.database_id).await?;

			let snapshot = common::history(&ctx.udb, branch_id).await?;

			common::assert_delta_txids(&snapshot, [1, 2, 3], "after three commits");
			common::assert_commit_txids(&snapshot, [1, 2, 3], "after three commits");
			common::assert_vtx_txids(&snapshot, [1, 2, 3], "after three commits");
			// PIDX maps each page to the latest txid that wrote it.
			common::assert_pidx(
				&snapshot,
				[(1, 1), (2, 2), (3, 2), (far_pgno, 3)],
				"after three commits",
			);
			// No compaction has run: no shards, no PITR intervals, no pins.
			common::assert_shard_versions(&snapshot, [], "after three commits");
			assert!(snapshot.pitr_intervals.is_empty());
			assert!(snapshot.pins.is_empty());
			assert_eq!(snapshot.hot_watermark_txid(), 0);
			assert_eq!(snapshot.head.as_ref().map(|head| head.head_txid), Some(3));
			// Two single-chunk deltas plus one for the far page commit.
			assert_eq!(snapshot.delta_chunk_rows, 3);

			Ok(())
		})
	})
	.await
}
