import { describe, expect, it } from "vitest";
import {
	deleteEntriesWithPrefix,
	MAX_CONCURRENT_DELETES,
	MAX_KV_BATCH_ENTRIES,
} from "../src/storage.js";
import {
	appendName,
	createEntry,
	createStorage,
	emptyLocation,
	InMemoryDriver,
	setEntry,
} from "../src/testing.js";

describe("Workflow Engine Storage delete fan-out", () => {
	// Records batchDelete sizes to assert keys are coalesced, not deleted one-by-one.
	class BatchDeleteRecordingDriver extends InMemoryDriver {
		batchSizes: number[] = [];
		singleDeletes = 0;

		override async batchDelete(keys: Uint8Array[]): Promise<void> {
			this.batchSizes.push(keys.length);
			await super.batchDelete(keys);
		}

		override async delete(key: Uint8Array): Promise<void> {
			this.singleDeletes++;
			await super.delete(key);
		}
	}

	it("clears a large history prefix in transaction-sized delete batches", async () => {
		const driver = new BatchDeleteRecordingDriver();
		driver.latency = 1;
		const storage = createStorage();
		const loopLocation = appendName(storage, emptyLocation(), "loop");

		// Span several batches so chunking is exercised.
		const entryCount = MAX_KV_BATCH_ENTRIES * 3 + 7;
		for (let i = 0; i < entryCount; i++) {
			const location = appendName(storage, loopLocation, `iter-${i}`);
			const entry = createEntry(location, {
				type: "step",
				data: { output: i },
			});
			setEntry(storage, location, entry);
		}

		await deleteEntriesWithPrefix(storage, driver, loopLocation);

		// All keys deleted via transaction-sized batches, no per-key fan-out.
		expect(driver.singleDeletes).toBe(0);
		expect(driver.batchSizes).toHaveLength(
			Math.ceil(entryCount / MAX_KV_BATCH_ENTRIES),
		);
		for (const size of driver.batchSizes) {
			expect(size).toBeLessThanOrEqual(MAX_KV_BATCH_ENTRIES);
		}
		expect(driver.batchSizes.reduce((a, b) => a + b, 0)).toBe(entryCount);
		expect(storage.history.entries.size).toBe(0);
	});

	// Tracks concurrent delete ops so the test can assert the fan-out stays bounded.
	class ConcurrencyTrackingDriver extends InMemoryDriver {
		inFlight = 0;
		peakInFlight = 0;

		async #track<T>(op: Promise<T>): Promise<T> {
			this.inFlight++;
			this.peakInFlight = Math.max(this.peakInFlight, this.inFlight);
			try {
				return await op;
			} finally {
				this.inFlight--;
			}
		}

		override batchDelete(keys: Uint8Array[]): Promise<void> {
			return this.#track(super.batchDelete(keys));
		}

		override deletePrefix(prefix: Uint8Array): Promise<void> {
			return this.#track(super.deletePrefix(prefix));
		}
	}

	it("bounds concurrent delete ops for a prune larger than the cap", async () => {
		const driver = new ConcurrencyTrackingDriver();
		driver.latency = 1;
		const storage = createStorage();
		const loopLocation = appendName(storage, emptyLocation(), "loop");

		// Enough keys to yield more batches than MAX_CONCURRENT_DELETES.
		const entryCount = MAX_CONCURRENT_DELETES * MAX_KV_BATCH_ENTRIES + 1;
		for (let i = 0; i < entryCount; i++) {
			const location = appendName(storage, loopLocation, `iter-${i}`);
			const entry = createEntry(location, {
				type: "step",
				data: { output: i },
			});
			setEntry(storage, location, entry);
		}

		await deleteEntriesWithPrefix(storage, driver, loopLocation);

		expect(driver.peakInFlight).toBeLessThanOrEqual(MAX_CONCURRENT_DELETES);
		expect(driver.peakInFlight).toBe(MAX_CONCURRENT_DELETES);
		expect(storage.history.entries.size).toBe(0);
	});
});
