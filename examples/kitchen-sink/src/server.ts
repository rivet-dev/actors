import { existsSync, readFileSync } from "node:fs";
import type { Server as HttpServer } from "node:http";
import { resolve } from "node:path";
import * as v8 from "node:v8";
import { serve } from "@hono/node-server";
import { serveStatic } from "@hono/node-server/serve-static";
import { Hono } from "hono";
import { registry } from "./index.ts";
import { resolveMode } from "./mode.ts";

const app = new Hono();
const port = Number.parseInt(process.env.PORT ?? "3000", 10);
const mode = resolveMode();

process.on("exit", (code) => {
	console.log(
		JSON.stringify({ kind: "process_exit", code, pid: process.pid }),
	);
});
if (process.env.SQLITE_MEMORY_SOAK_DIAGNOSTICS === "1") {
	for (const signal of ["SIGINT", "SIGTERM"] as const) {
		process.on(signal, () => {
			console.log(
				JSON.stringify({
					kind: "process_signal",
					signal,
					pid: process.pid,
					ppid: process.ppid,
					timestamp: new Date().toISOString(),
				}),
			);
			process.exit(signal === "SIGINT" ? 130 : 143);
		});
	}
}
process.on("beforeExit", (code) => {
	console.log(
		JSON.stringify({ kind: "process_before_exit", code, pid: process.pid }),
	);
});
process.on("uncaughtException", (error) => {
	console.error(
		JSON.stringify({
			kind: "uncaught_exception",
			error: error.stack ?? error.message,
		}),
	);
});
process.on("unhandledRejection", (reason) => {
	console.error(
		JSON.stringify({
			kind: "unhandled_rejection",
			error:
				reason instanceof Error
					? (reason.stack ?? reason.message)
					: String(reason),
		}),
	);
});

async function memoryBreakdown(forceGc: boolean) {
	const gc = (globalThis as typeof globalThis & { gc?: () => void }).gc;
	if (forceGc && typeof gc === "function") gc();

	const memory = process.memoryUsage();
	const heap = v8.getHeapStatistics();
	const spaces = v8.getHeapSpaceStatistics();
	const nativeNonV8Estimate = Math.max(0, memory.rss - heap.total_heap_size);

	return {
		pid: process.pid,
		timestamp: new Date().toISOString(),
		uptimeSeconds: process.uptime(),
		gcRequested: forceGc,
		gcAvailable: typeof gc === "function",
		process: {
			rssBytes: memory.rss,
			heapTotalBytes: memory.heapTotal,
			heapUsedBytes: memory.heapUsed,
			externalBytes: memory.external,
			arrayBuffersBytes: memory.arrayBuffers,
		},
		v8: {
			totalHeapSizeBytes: heap.total_heap_size,
			usedHeapSizeBytes: heap.used_heap_size,
			heapSizeLimitBytes: heap.heap_size_limit,
			mallocedMemoryBytes: heap.malloced_memory,
			externalMemoryBytes: heap.external_memory,
			peakMallocedMemoryBytes: heap.peak_malloced_memory,
			spaces: spaces.map((space) => ({
				name: space.space_name,
				sizeBytes: space.space_size,
				usedBytes: space.space_used_size,
				availableBytes: space.space_available_size,
				physicalSizeBytes: space.physical_space_size,
			})),
		},
		estimates: {
			jsHeapResidentBytes: memory.heapTotal,
			jsHeapUsedBytes: memory.heapUsed,
			v8ExternalBytes: memory.external,
			nativeNonV8ResidentEstimateBytes: nativeNonV8Estimate,
		},
		resourceUsage: process.resourceUsage(),
	};
}

function _requestHeaders(headers: Headers) {
	const entries: Array<[string, string]> = [];
	headers.forEach((value, key) => {
		entries.push([
			key,
			key === "authorization" || key === "x-rivet-token"
				? "<redacted>"
				: value,
		]);
	});
	return Object.fromEntries(entries);
}

app.get("/debug/memory", async (c) => {
	const forceGc = c.req.query("gc") === "1";
	return c.json(await memoryBreakdown(forceGc));
});

app.get("/health", (c) => c.json({ ok: true }));
app.get("/metadata", () => registry.routes.metadata());
app.get("/metrics", (c) => registry.routes.prometheusMetrics(c.req.raw));

app.post("/debug/heap-snapshot", (c) => {
	if (process.env.SQLITE_MEMORY_SOAK_DIAGNOSTICS !== "1") {
		return c.json({ error: "disabled" }, 404);
	}

	const path = c.req.query("path");
	if (!path) {
		return c.json({ error: "missing path" }, 400);
	}

	const writtenPath = v8.writeHeapSnapshot(path);
	return c.json({ path: writtenPath });
});

app.use("*", async (_c, next) => {
	const _startedAt = Date.now();
	await next();
	// console.log(
	// 	JSON.stringify({
	// 		kind: "request",
	// 		method: c.req.method,
	// 		path: new URL(c.req.url).pathname,
	// 		headers: requestHeaders(c.req.raw.headers),
	// 		status: c.res.status,
	// 		durationMs: Date.now() - startedAt,
	// 	}),
	// );
});

// Only wire the serverless handler in serverless modes. In `serverful` mode
// the runner connects via `registry.start()` and any stray hit on
// `/api/rivet/start` would spin up a second envoy alongside the persistent
// one.
if (mode === "serverful") {
	registry.start();
} else {
	app.all("/api/rivet/*", (c) => registry.handler(c.req.raw));
	app.all("/api/rivet", (c) => registry.handler(c.req.raw));
}

// Serve the built frontend when it is present. The Vite build emits `dist/`,
// which only exists in production images, so dev runs skip this branch.
const distDir = "dist";
const indexPath = resolve(process.cwd(), distDir, "index.html");
if (existsSync(indexPath)) {
	app.use("/*", serveStatic({ root: distDir }));
	const indexHtml = readFileSync(indexPath, "utf8");
	app.get("/*", (c) => {
		const path = new URL(c.req.url).pathname;
		const last = path.slice(path.lastIndexOf("/") + 1);
		// Fall through to 404 for asset-like paths so missing files do not
		// resolve to the SPA shell.
		if (last.includes(".")) return c.notFound();
		return c.html(indexHtml);
	});
}

const server = serve({ fetch: app.fetch, port }, () => {
	if (mode === "serverful") {
		console.log(
			`kitchen sink (serverful) listening on http://127.0.0.1:${port}`,
		);
	} else {
		console.log(
			`kitchen sink (${mode}) listening on http://127.0.0.1:${port}/api/rivet`,
		);
	}
});
const httpServer = server as unknown as HttpServer;
httpServer.requestTimeout = 0;
httpServer.headersTimeout = 0;
httpServer.keepAliveTimeout = 0;
httpServer.timeout = 0;
