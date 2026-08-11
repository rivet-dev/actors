import { setup } from "rivetkit";
// Actions
import { inputActor } from "./actors/actions/action-inputs.ts";
import {
	defaultTimeoutActor,
	longTimeoutActor,
	shortTimeoutActor,
	syncTimeoutActor,
} from "./actors/actions/action-timeout.ts";
import {
	asyncActionActor,
	promiseActor,
	syncActionActor,
} from "./actors/actions/action-types.ts";
import {
	customTimeoutActor,
	errorHandlingActor,
} from "./actors/actions/error-handling.ts";
// AI
import { aiAgent } from "./actors/ai/ai-agent.ts";
// Connections
import { connStateActor } from "./actors/connections/conn-state.ts";
import { rejectConnectionActor } from "./actors/connections/reject-connection.ts";
import { requestAccessActor } from "./actors/connections/request-access.ts";
import { counterWithParams } from "./actors/counter/conn-params.ts";
// Counter
import { counter } from "./actors/counter/counter.ts";
import { counterConn } from "./actors/counter/counter-conn.ts";
import { counterWithLifecycle } from "./actors/counter/lifecycle.ts";
import { pingPongCounter } from "./actors/counter/ping-pong-counter.ts";
import { rawFetchCounter } from "./actors/http/raw-fetch-counter.ts";
// HTTP
import {
	rawHttpActor,
	rawHttpHonoActor,
	rawHttpNoHandlerActor,
	rawHttpVoidReturnActor,
} from "./actors/http/raw-http.ts";
import { rawHttpRequestPropertiesActor } from "./actors/http/raw-http-request-properties.ts";
import {
	rawWebSocketActor,
	rawWebSocketBinaryActor,
} from "./actors/http/raw-websocket.ts";
import { rawWebSocketChatRoom } from "./actors/http/raw-websocket-chat-room.ts";
import { rawWebSocketServerlessSmoke } from "./actors/http/raw-websocket-serverless-smoke.ts";
import { tunnelStress } from "./actors/http/tunnel-stress.ts";
// Inter-actor
import {
	checkout,
	inventory,
} from "./actors/inter-actor/cross-actor-actions.ts";
import { destroyActor, destroyObserver } from "./actors/lifecycle/destroy.ts";
import { hibernationActor } from "./actors/lifecycle/hibernation.ts";
// Lifecycle
import {
	runWithEarlyExit,
	runWithError,
	runWithoutHandler,
	runWithQueueConsumer,
	runWithTicks,
} from "./actors/lifecycle/run.ts";
import { scheduled } from "./actors/lifecycle/scheduled.ts";
import {
	sleep,
	sleepWithLongRpc,
	sleepWithNoSleepOption,
	sleepWithRawHttp,
	sleepWithRawWebSocket,
} from "./actors/lifecycle/sleep.ts";
// Queues
import { worker } from "./actors/queue/worker.ts";
import { workerTimeout } from "./actors/queue/worker-timeout.ts";
// State
import { onStateChangeActor } from "./actors/state/actor-onstatechange.ts";
import { kvActor } from "./actors/state/kv.ts";
import {
	largePayloadActor,
	largePayloadConnActor,
} from "./actors/state/large-payloads.ts";
import { metadataActor } from "./actors/state/metadata.ts";
import { parallelismTest } from "./actors/state/parallelism-test.ts";
import { sqliteDrizzleActor } from "./actors/state/sqlite-drizzle/mod.ts";
import { sqliteRawActor } from "./actors/state/sqlite-raw.ts";
import {
	driverCtxActor,
	dynamicVarActor,
	nestedVarActor,
	staticVarActor,
	uniqueVarActor,
} from "./actors/state/vars.ts";
// Testing
import { churnDb } from "./actors/testing/churn-db.ts";
import { growDb } from "./actors/testing/grow-db.ts";
import { inlineClientActor } from "./actors/testing/inline-client.ts";
import { loadTestAgent } from "./actors/testing/load-test-agent.ts";
import { loadTestAgent2 } from "./actors/testing/load-test-agent-2.ts";
import { mockAgenticLoop } from "./actors/testing/mock-agentic-loop.ts";
import { rawSqliteFuzzer } from "./actors/testing/raw-sqlite-fuzzer.ts";
import { sigtermSleepProbe } from "./actors/testing/sigterm-sleep-probe.ts";
import { sleepCloseFuzz } from "./actors/testing/sleep-close-fuzz.ts";
import { slowReconnectActor } from "./actors/testing/slow-reconnect-actor.ts";
import { sqliteColdStartBench } from "./actors/testing/sqlite-cold-start-bench.ts";
import { sqliteMemoryPressure } from "./actors/testing/sqlite-memory-pressure.ts";
import { sqliteRealworldBench } from "./actors/testing/sqlite-realworld-bench.ts";
import { testCounter } from "./actors/testing/test-counter.ts";
import { testCounterSqlite } from "./actors/testing/test-counter-sqlite.ts";
import { testSqliteBench } from "./actors/testing/test-sqlite-bench.ts";
import { testSqliteLoad } from "./actors/testing/test-sqlite-load.ts";
import { approval } from "./actors/workflow/approval.ts";
import { batch } from "./actors/workflow/batch.ts";
import { dashboard } from "./actors/workflow/dashboard.ts";
import {
	workflowHistoryFailed,
	workflowHistoryFull,
	workflowHistoryInProgress,
	workflowHistoryJoin,
	workflowHistoryLoop,
	workflowHistoryRace,
	workflowHistoryRetrying,
	workflowHistorySimple,
} from "./actors/workflow/history-examples.ts";
import { order } from "./actors/workflow/order.ts";
import { payment } from "./actors/workflow/payment.ts";
import { race } from "./actors/workflow/race.ts";
import { timer } from "./actors/workflow/timer.ts";
// Workflows
import {
	workflowCounterActor,
	workflowQueueActor,
	workflowQueueTimeoutActor,
	workflowSleepActor,
} from "./actors/workflow/workflow-fixtures.ts";
import { resolveMode } from "./mode.ts";

function numberFromEnv(name: string, fallback: number): number {
	const value = process.env[name];
	if (value === undefined || value === "") return fallback;

	const parsed = Number(value);
	if (!Number.isFinite(parsed)) {
		throw new Error(`${name} must be a finite number`);
	}

	return parsed;
}

function serverlessPoolConfig() {
	// Only the local serverless mode self-registers its pool with the engine.
	// In the deployed `serverless` mode the pool is configured externally on
	// the engine cluster, and the `serverful` mode uses a long-lived runner
	// connection rather than a serverless pool.
	if (resolveMode() !== "serverless-local") return undefined;

	const url =
		process.env.RIVET_SERVERLESS_URL ??
		process.env.KITCHEN_SINK_SERVERLESS_URL ??
		"http://127.0.0.1:3000/api/rivet";

	return {
		name: process.env.RIVET_POOL,
		url,
		requestLifespan: numberFromEnv(
			"RIVET_SERVERLESS_REQUEST_LIFESPAN",
			15 * 60,
		),
		drainGracePeriod: numberFromEnv(
			"RIVET_SERVERLESS_DRAIN_GRACE_PERIOD",
			15 * 60,
		),
		metadataPollInterval: numberFromEnv(
			"RIVET_SERVERLESS_METADATA_POLL_INTERVAL_MS",
			1000,
		),
		metadata: {
			source: "kitchen-sink",
			smoke: "raw-websocket-serverless",
		},
	};
}

export const registry = setup({
	sqlite:
		process.env.RIVET_KITCHEN_SINK_REMOTE_SQLITE === "1"
			? { backend: "remote" }
			: undefined,
	configurePool: serverlessPoolConfig(),
	serverless: {
		publicToken:
			process.env.RIVET_PUBLIC_TOKEN ?? process.env.RIVET_TOKEN ?? "dev",
		maxStartPayloadBytes: numberFromEnv(
			"RIVET_SERVERLESS_MAX_START_PAYLOAD_BYTES",
			16 * 1024 * 1024,
		),
	},
	use: {
		// Overview + state basics
		counter,
		counterConn,
		counterWithParams,
		counterWithLifecycle,
		pingPongCounter,
		// Core API
		inputActor,
		syncActionActor,
		asyncActionActor,
		promiseActor,
		shortTimeoutActor,
		longTimeoutActor,
		defaultTimeoutActor,
		syncTimeoutActor,
		customTimeoutActor,
		errorHandlingActor,
		// State and storage
		onStateChangeActor,
		metadataActor,
		staticVarActor,
		nestedVarActor,
		dynamicVarActor,
		uniqueVarActor,
		driverCtxActor,
		kvActor,
		largePayloadActor,
		largePayloadConnActor,
		sqliteRawActor,
		sqliteDrizzleActor,
		parallelismTest,
		// Realtime and connections
		connStateActor,
		rejectConnectionActor,
		requestAccessActor,
		// HTTP and WebSocket
		rawHttpActor,
		rawHttpNoHandlerActor,
		rawHttpVoidReturnActor,
		rawHttpHonoActor,
		rawHttpRequestPropertiesActor,
		rawWebSocketActor,
		rawWebSocketBinaryActor,
		rawFetchCounter,
		rawWebSocketChatRoom,
		rawWebSocketServerlessSmoke,
		tunnelStress,
		// Lifecycle and scheduling
		runWithTicks,
		runWithQueueConsumer,
		runWithEarlyExit,
		runWithError,
		runWithoutHandler,
		sleep,
		sleepWithLongRpc,
		sleepWithNoSleepOption,
		sleepWithRawHttp,
		sleepWithRawWebSocket,
		scheduled,
		destroyActor,
		destroyObserver,
		hibernationActor,
		// Queues
		worker,
		workerTimeout,
		// Workflows
		timer,
		order,
		batch,
		approval,
		dashboard,
		race,
		payment,
		workflowHistorySimple,
		workflowHistoryLoop,
		workflowHistoryJoin,
		workflowHistoryRace,
		workflowHistoryFull,
		workflowHistoryInProgress,
		workflowHistoryRetrying,
		workflowHistoryFailed,
		workflowCounterActor,
		workflowQueueActor,
		workflowSleepActor,
		workflowQueueTimeoutActor,
		// Inter-actor
		inventory,
		checkout,
		// Testing fixtures
		inlineClientActor,
		testCounter,
		testCounterSqlite,
		testSqliteLoad,
		testSqliteBench,
		sqliteColdStartBench,
		sqliteRealworldBench,
		rawSqliteFuzzer,
		sqliteMemoryPressure,
		growDb,
		churnDb,
		mockAgenticLoop,
		sleepCloseFuzz,
		loadTestAgent,
		loadTestAgent2,
		sigtermSleepProbe,
		slowReconnectActor,
		// AI
		aiAgent,
	},
});
