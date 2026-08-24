import type { StreamChunksResponse, StreamInfoResponse } from "@workflow/world";
import { event, type Type } from "rivetkit";
import { decodeValue, encodeValue } from "../codec.js";
import { withActorTransaction, type Ctx } from "./shared.js";

function envInteger(name: string, fallback: number, min: number) {
	const value = Number(process.env[name]);
	return Number.isFinite(value) ? Math.max(min, Math.floor(value)) : fallback;
}

export const STREAM_TTL_MS = envInteger(
	"RIVET_WORLD_RIVET_STREAM_TTL_MS",
	86_400_000,
	0,
);

export const streamAppended: Type<{
	streamId: string;
	runId: string;
	chunkId: string;
	eof: boolean;
}> = event();

type StreamContext = Ctx & {
	key: readonly string[];
	broadcast(name: "streamAppended", value: unknown): void;
};

function assertRunKey(c: StreamContext, runId: string) {
	if (c.key[0] !== runId) throw new Error("workflowRun actor key must match runId");
}

function decodeBase64Chunk(value: string) {
	return new Uint8Array(Buffer.from(value, "base64"));
}

let broadcastCounter = 0;
function shouldDropBroadcast(eof: boolean) {
	if (eof) return false;
	const rate = Number(process.env.RIVET_WORLD_RIVET_INJECT_BROADCAST_DROP_RATE);
	if (!Number.isFinite(rate) || rate <= 0) return false;
	const r = Math.min(rate, 1);
	const n = broadcastCounter++;
	return Math.floor((n + 1) * r) > Math.floor(n * r);
}

async function injectReadDelay() {
	const ms = Number(process.env.RIVET_WORLD_RIVET_INJECT_STREAM_READ_DELAY_MS);
	if (Number.isFinite(ms) && ms > 0) {
		await new Promise((resolve) => setTimeout(resolve, ms));
	}
}

async function cleanupExpired(c: Ctx, streamId: string) {
	if (STREAM_TTL_MS === 0) return 0;
	const expired = await c.db.execute(
		`SELECT 1 FROM stream_chunks WHERE stream_id = ? AND eof = 1
		 AND created_at <= ? LIMIT 1`,
		streamId,
		Date.now() - STREAM_TTL_MS,
	);
	if (expired.length === 0) return 0;
	return (
		await c.db.execute(
			"DELETE FROM stream_chunks WHERE stream_id = ? RETURNING sequence",
			streamId,
		)
	).length;
}

export async function writeStream(
	c: StreamContext,
	streamId: string,
	runId: string,
	chunkBase64: string,
	eof: boolean,
) {
	assertRunKey(c, runId);
	await cleanupExpired(c, streamId);
	const result = await withActorTransaction(c, async (tx) => {
		const closed = await tx.db.execute(
			"SELECT sequence FROM stream_chunks WHERE stream_id = ? AND eof = 1 LIMIT 1",
			streamId,
		);
		if (closed[0]) {
			if (eof) return { sequence: Number(closed[0].sequence), inserted: false };
			throw new Error(`stream "${streamId}" is already closed`);
		}
		const next = await tx.db.execute(
			"SELECT COALESCE(MAX(sequence), -1) + 1 AS sequence FROM stream_chunks WHERE stream_id = ?",
			streamId,
		);
		const value = Number(next[0]?.sequence ?? 0);
		await tx.db.execute(
			`INSERT INTO stream_chunks (stream_id, sequence, chunk_data, eof, created_at)
			 VALUES (?, ?, ?, ?, ?)`,
			streamId,
			value,
			encodeValue(decodeBase64Chunk(chunkBase64)),
			eof ? 1 : 0,
			Date.now(),
		);
		await tx.db.execute(
			"INSERT OR IGNORE INTO run_streams (stream_id) VALUES (?)",
			streamId,
		);
		return { sequence: value, inserted: true };
	});
	const chunkId = String(result.sequence);
	if (result.inserted && !shouldDropBroadcast(eof)) {
		c.broadcast("streamAppended", { streamId, runId, chunkId, eof });
	}
	return { chunkId };
}

export async function getStreamChunks(
	c: StreamContext,
	streamId: string,
	runId: string,
	startIndex = 0,
	limit = 100,
): Promise<StreamChunksResponse> {
	assertRunKey(c, runId);
	await cleanupExpired(c, streamId);
	const rows = await c.db.execute(
		`SELECT sequence, chunk_data FROM stream_chunks
		 WHERE stream_id = ? AND eof = 0 AND sequence >= ?
		 ORDER BY sequence ASC LIMIT ?`,
		streamId,
		startIndex,
		limit + 1,
	);
	const hasMore = rows.length > limit;
	const values = rows.slice(0, limit);
	const data = values.map((row) => ({
		index: Number(row.sequence),
		data: decodeValue<Uint8Array>(row.chunk_data) ?? new Uint8Array(),
	}));
	const closed = await c.db.execute(
		"SELECT 1 FROM stream_chunks WHERE stream_id = ? AND eof = 1 LIMIT 1",
		streamId,
	);
	await injectReadDelay();
	const nextSequence = values.length
		? Number(values.at(-1)?.sequence) + 1
		: startIndex;
	return {
		data,
		cursor: hasMore
			? Buffer.from(JSON.stringify({ i: nextSequence })).toString("base64")
			: null,
		hasMore,
		done: !hasMore && closed.length > 0,
	};
}

export async function getStreamInfo(
	c: StreamContext,
	streamId: string,
	runId: string,
): Promise<StreamInfoResponse> {
	assertRunKey(c, runId);
	await cleanupExpired(c, streamId);
	const row = (
		await c.db.execute(
			`SELECT SUM(CASE WHEN eof = 0 THEN 1 ELSE 0 END) AS chunk_count,
			 MAX(eof) AS done FROM stream_chunks WHERE stream_id = ?`,
			streamId,
		)
	)[0];
	const count = Number(row?.chunk_count ?? 0);
	return { tailIndex: count - 1, done: Number(row?.done ?? 0) === 1 };
}

export async function listStreams(c: StreamContext, runId: string) {
	assertRunKey(c, runId);
	return (
		await c.db.execute(`SELECT r.stream_id FROM run_streams r
			WHERE EXISTS (SELECT 1 FROM stream_chunks c WHERE c.stream_id = r.stream_id)
			ORDER BY r.stream_id`)
	).map((row) => String(row.stream_id));
}

export async function cleanupExpiredStreams(c: Ctx) {
	if (STREAM_TTL_MS === 0) return;
	const cutoff = Date.now() - STREAM_TTL_MS;
	const streams = await c.db.execute(
		`SELECT DISTINCT stream_id FROM stream_chunks
		 WHERE eof = 1 AND created_at <= ?
		 ORDER BY stream_id LIMIT 32`,
		cutoff,
	);
	for (const row of streams) {
		const streamId = String(row.stream_id);
		await c.db.execute("DELETE FROM stream_chunks WHERE stream_id = ?", streamId);
		await c.db.execute("DELETE FROM run_streams WHERE stream_id = ?", streamId);
	}
}

export async function expireClosedStreamForTesting(
	c: StreamContext,
	streamId: string,
	runId: string,
) {
	assertRunKey(c, runId);
	if (process.env.RIVET_WORLD_RIVET_TESTING !== "1") {
		throw new Error("expireClosedForTesting requires test mode");
	}
	await c.db.execute(
		"UPDATE stream_chunks SET created_at = ? WHERE stream_id = ? AND eof = 1",
		Date.now() - STREAM_TTL_MS - 1_000,
		streamId,
	);
	return { deletedChunks: await cleanupExpired(c, streamId) };
}
