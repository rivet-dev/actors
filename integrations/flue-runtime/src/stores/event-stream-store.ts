import type {
	EventStreamMeta,
	EventStreamReadResult,
	EventStreamStore,
} from '@flue/runtime/adapter-kit';
import type { AsyncSqlDb, AsyncSqlRunner } from './async-db.js';
import { DEFAULT_READ_LIMIT, MAX_READ_LIMIT } from './constants.js';
import { clampLimit } from './helpers.js';

const COMPONENT_PAD = 16;
const ZERO_COMPONENT = '0'.repeat(COMPONENT_PAD);

export function createAsyncEventStreamStore(db: AsyncSqlDb): EventStreamStore {
	return new AsyncEventStreamStore(db);
}

export function formatOffset(seq: number): string {
	if (seq === -1) return '-1';
	return `${ZERO_COMPONENT}_${String(seq).padStart(COMPONENT_PAD, '0')}`;
}

export function parseOffset(offset: string): number {
	if (offset === '-1') return -1;
	const match = /^\d+_(\d+)$/.exec(offset);
	const sequence = match?.[1];
	if (!sequence) throw new Error(`[flue] Invalid stream offset: "${offset}".`);
	return parseInt(sequence, 10);
}

class AsyncEventStreamStore implements EventStreamStore {
	private listeners = new Map<string, Set<() => void>>();
	private pendingAppends = new Map<string, Promise<void>>();
	private db: AsyncSqlDb;

	constructor(db: AsyncSqlDb) {
		this.db = db;
	}

	async createStream(path: string): Promise<void> {
		await this.db.query(`INSERT OR IGNORE INTO flue_event_streams (path) VALUES (?)`, [path]);
	}

	async appendEvent(path: string, event: unknown): Promise<string> {
		const previous = this.pendingAppends.get(path) ?? Promise.resolve();
		const append = previous.then(async () => {
			const data = JSON.stringify(event);
			const seq = await this.db.transaction(async (tx) => {
				const updated = await tx.query(
					`UPDATE flue_event_streams
					 SET next_offset = next_offset + 1
					 WHERE path = ? AND closed = 0
					 RETURNING next_offset`,
					[path],
				);
				if (updated.length === 0) {
					const meta = await getStreamMetaFromRunner(tx, path);
					if (!meta) throw new Error(`[flue] Event stream "${path}" does not exist.`);
					throw new Error(`[flue] Event stream "${path}" is closed.`);
				}
				const offset = Number(updated[0]?.next_offset) - 1;
				await tx.query(
					`INSERT INTO flue_event_stream_entries (path, seq, data) VALUES (?, ?, ?)`,
					[path, offset, data],
				);
				return offset;
			});
			this.notifyListeners(path);
			return formatOffset(seq);
		});
		const settled = append.then(
			() => undefined,
			() => undefined,
		);
		this.pendingAppends.set(path, settled);
		try {
			return await append;
		} finally {
			if (this.pendingAppends.get(path) === settled) {
				this.pendingAppends.delete(path);
			}
		}
	}

	async appendEventOnce(path: string, key: string, event: unknown): Promise<string> {
		const data = JSON.stringify(event);
		const result = await this.db.transaction(async (tx) => {
			const existing = await tx.query(
				`SELECT seq, data FROM flue_event_stream_keys WHERE path = ? AND key = ? LIMIT 1`,
				[path, key],
			);
			if (existing[0]) {
				if (existing[0].data !== data) {
					throw new Error(`[flue] Event key "${key}" already has a conflicting payload.`);
				}
				return { offset: Number(existing[0].seq), appended: false };
			}
			const updated = await tx.query(
				`UPDATE flue_event_streams SET next_offset = next_offset + 1
				 WHERE path = ? AND closed = 0 RETURNING next_offset`,
				[path],
			);
			if (!updated[0]) {
				const meta = await getStreamMetaFromRunner(tx, path);
				if (!meta) throw new Error(`[flue] Event stream "${path}" does not exist.`);
				throw new Error(`[flue] Event stream "${path}" is closed.`);
			}
			const offset = Number(updated[0].next_offset) - 1;
			await tx.query(
				`INSERT INTO flue_event_stream_entries (path, seq, data) VALUES (?, ?, ?)`,
				[path, offset, data],
			);
			await tx.query(
				`INSERT INTO flue_event_stream_keys (path, key, seq, data) VALUES (?, ?, ?, ?)`,
				[path, key, offset, data],
			);
			return { offset, appended: true };
		});
		if (result.appended) this.notifyListeners(path);
		return formatOffset(result.offset);
	}

	async readEvents(
		path: string,
		opts?: { offset?: string; limit?: number },
	): Promise<EventStreamReadResult> {
		const meta = await this.getStreamMeta(path);
		if (!meta) return { events: [], nextOffset: formatOffset(-1), upToDate: true, closed: false };

		const rawOffset = opts?.offset ?? '-1';
		const limit = clampLimit(opts?.limit, DEFAULT_READ_LIMIT, MAX_READ_LIMIT);
		let startAfter: number;
		if (rawOffset === '-1') {
			startAfter = -1;
		} else if (rawOffset === 'now') {
			return { events: [], nextOffset: meta.nextOffset, upToDate: true, closed: meta.closed };
		} else {
			startAfter = parseOffset(rawOffset);
		}

		const rows = await this.db.query(
			`SELECT seq, data FROM flue_event_stream_entries
			 WHERE path = ? AND seq > ?
			 ORDER BY seq ASC
			 LIMIT ?`,
			[path, startAfter, limit + 1],
		);
		const page = rows.slice(0, limit);
		const events = page.map((row) => ({
			data: JSON.parse(String(row.data)) as unknown,
			offset: formatOffset(Number(row.seq)),
		}));
		const lastSeq = page.length > 0 ? Number(page.at(-1)?.seq) : -1;
		return {
			events,
			nextOffset: events.length > 0 ? formatOffset(lastSeq) : formatOffset(startAfter),
			upToDate: rows.length <= limit,
			closed: meta.closed,
		};
	}

	async closeStream(path: string): Promise<void> {
		await this.db.query(`UPDATE flue_event_streams SET closed = 1 WHERE path = ?`, [path]);
		this.notifyListeners(path);
	}

	async getStreamMeta(path: string): Promise<EventStreamMeta | null> {
		return getStreamMetaFromRunner(this.db, path);
	}

	subscribe(path: string, listener: () => void): () => void {
		let bucket = this.listeners.get(path);
		if (!bucket) {
			bucket = new Set();
			this.listeners.set(path, bucket);
		}
		bucket.add(listener);
		const listeners = bucket;
		return () => {
			listeners.delete(listener);
			if (listeners.size === 0) this.listeners.delete(path);
		};
	}

	private notifyListeners(path: string): void {
		const bucket = this.listeners.get(path);
		if (!bucket) return;
		for (const listener of [...bucket]) {
			try {
				listener();
			} catch {
				// Listener failures are intentionally isolated from writes.
			}
		}
	}
}

async function getStreamMetaFromRunner(
	runner: AsyncSqlRunner,
	path: string,
): Promise<EventStreamMeta | null> {
	const rows = await runner.query(
		`SELECT next_offset, closed FROM flue_event_streams WHERE path = ? LIMIT 1`,
		[path],
	);
	const row = rows[0];
	if (!row) return null;
	const writeHead = Number(row.next_offset);
	return {
		nextOffset: formatOffset(writeHead - 1),
		closed: Boolean(row.closed),
	};
}
