import { setup } from "rivetkit";
// This file is the single static registry source for the driver fixtures.
// Static runs import this registry directly, and dynamic runs reuse its types and actor metadata.
import {
	accessControlActor,
	accessControlNoQueuesActor,
} from "./access-control";

import { inputActor } from "./action-inputs";
import {
	defaultTimeoutActor,
	longTimeoutActor,
	shortTimeoutActor,
	syncTimeoutActor,
} from "./action-timeout";
import {
	asyncActionActor,
	concurrentActionActor,
	nestedActionActor,
	promiseActor,
	syncActionActor,
} from "./action-types";
import {
	dbActorDrizzleMigration,
	dbActorDrizzleMigrationRollback,
} from "./actor-db-drizzle-transaction";
import {
	actorRuntimeSocketWithoutDb,
	dbActorManualWarningsDisabled,
	dbActorRaw,
	dbActorRuntimeSocketDisabled,
	dbRemoteLifecycleProbe,
} from "./actor-db-raw";
import { onStateChangeActor } from "./actor-onstatechange";
import { connErrorSerializationActor } from "./conn-error-serialization";
import { counterWithParams } from "./conn-params";
import { connPreflightVisibilityActor } from "./conn-preflight-visibility";
import { connStateActor } from "./conn-state";
// Import actors from individual files
import { counter } from "./counter";
import { counterConn } from "./counter-conn";
import {
	dbInitOrderCreateStateActor,
	dbInitOrderCreateVarsActor,
	dbInitOrderObserver,
	dbInitOrderOnCreateActor,
	dbInitOrderOnDestroyActor,
	dbInitOrderOnSleepActor,
	dbInitOrderOnWakeActor,
} from "./db-init-order";
import {
	dbLifecycle,
	dbLifecycleFailing,
	dbLifecycleObserver,
} from "./db-lifecycle";
import { dbPragmaMigrationActor } from "./db-pragma-migration";
import { dbStressActor } from "./db-stress";
import {
	destroyAbortSignalActor,
	destroyActor,
	destroyObserver,
} from "./destroy";
import { customTimeoutActor, errorHandlingActor } from "./error-handling";
import { fileSystemHibernationCleanupActor } from "./file-system-hibernation-cleanup";
import { hibernationActor, hibernationSleepWindowActor } from "./hibernation";
import { kvActor } from "./kv";
import { largePayloadActor, largePayloadConnActor } from "./large-payloads";
import { counterWithLifecycle } from "./lifecycle";
import {
	beforeConnectGenericErrorActor,
	beforeConnectRejectActor,
	beforeConnectTimeoutActor,
	stateChangeRecursionActor,
	stateChangeReentrantMutationActor,
} from "./lifecycle-hooks";
import { metadataActor } from "./metadata";
import {
	manyQueueActionParentActor,
	manyQueueChildActor,
	manyQueueRunParentActor,
	queueActor,
	queueLimitedActor,
} from "./queue";
import {
	rawHttpActor,
	rawHttpHonoActor,
	rawHttpNoHandlerActor,
	rawHttpVoidReturnActor,
} from "./raw-http";
import { rawHttpRequestPropertiesActor } from "./raw-http-request-properties";
import {
	rawWebSocketActor,
	rawWebSocketAsyncOpenActor,
	rawWebSocketBinaryActor,
	rawWebSocketConnContextActor,
} from "./raw-websocket";
import { rejectConnectionActor } from "./reject-connection";
import { requestAccessActor } from "./request-access";
import {
	runIgnoresAbortStopTimeout,
	runSelfInitiatedDestroy,
	runSelfInitiatedSleep,
	runWithEarlyExit,
	runWithError,
	runWithoutHandler,
	runWithQueueConsumer,
	runWithTicks,
	runWithWakeDeadline,
} from "./run";
import { saveStateActor, saveStateObserver } from "./save-state";
import { scheduled } from "./scheduled";
import { scheduledDb } from "./scheduled-db";
import {
	counterWaitUntilProbe,
	sleep,
	sleepAbortListenerVarsActor,
	sleepRawWsAddEventListenerClose,
	sleepRawWsAddEventListenerMessage,
	sleepRawWsDelayedSendOnSleep,
	sleepRawWsOnClose,
	sleepRawWsOnMessage,
	sleepRawWsSendOnSleep,
	sleepRawWsVarsExceedsGrace,
	sleepWaitUntilVarsDuringGrace,
	sleepWithLongRpc,
	sleepWithNoSleepOption,
	sleepWithRawHttp,
	sleepWithRawWebSocket,
	sleepWithWaitUntilInOnWake,
	sleepWithWaitUntilMessage,
} from "./sleep";
import {
	sleepEnqueue,
	sleepKeepAwakeUntilIdle,
	sleepNestedWaitUntil,
	sleepOnSleepThrows,
	sleepScheduleAfter,
	sleepWaitUntil,
	sleepWaitUntilRejects,
	sleepWaitUntilState,
	sleepWithDb,
	sleepWithDbAction,
	sleepWithDbConn,
	sleepWithRawWs,
	sleepWithRawWsCloseDb,
	sleepWithRawWsCloseDbListener,
	sleepWithSlowScheduledDb,
	sleepWsActiveDbExceedsGrace,
	sleepWsConcurrentDbExceedsGrace,
	sleepWsMessageExceedsGrace,
} from "./sleep-db";
import { lifecycleObserver, startStopRaceActor } from "./start-stop-race";
import { stateZodCoercionActor } from "./state-zod-coercion";
import { statelessActor } from "./stateless";
import {
	driverCtxActor,
	dynamicVarActor,
	nestedVarActor,
	staticVarActor,
	uniqueVarActor,
} from "./vars";
import { warmupActor } from "./warmup";
import {
	workflowAccessActor,
	workflowCompleteActor,
	workflowCounterActor,
	workflowDestroyActor,
	workflowErrorHookActor,
	workflowErrorHookEffectsActor,
	workflowErrorHookSleepActor,
	workflowFailedStepActor,
	workflowNestedJoinActor,
	workflowNestedLoopActor,
	workflowNestedRaceActor,
	workflowQueueActor,
	workflowReplayActor,
	workflowRunningStepActor,
	workflowSleepActor,
	workflowSpawnChildActor,
	workflowSpawnParentActor,
	workflowStepRollForwardActor,
	workflowStopTeardownActor,
	workflowTryActor,
} from "./workflow";

let agentOsTestActor:
	| Awaited<typeof import("./agent-os")>["agentOsTestActor"]
	| undefined;

try {
	({ agentOsTestActor } = await import("./agent-os"));
} catch (error) {
	if (!(error instanceof Error) || !error.message.includes("agent-os")) {
		throw error;
	}
}

// Consolidated setup with all actors
export const registry = setup({
	use: {
		// From counter.ts
		counter,
		// From counter-conn.ts
		counterConn,
		// From lifecycle.ts
		counterWithLifecycle,
		// From scheduled.ts
		scheduled,
		// From db-stress.ts
		dbStressActor,
		// From scheduled-db.ts
		scheduledDb,
		// From sleep.ts
		sleep,
		sleepWithLongRpc,
		sleepWithRawHttp,
		sleepWithRawWebSocket,
		sleepWithNoSleepOption,
		sleepWithWaitUntilMessage,
		sleepRawWsAddEventListenerMessage,
		sleepRawWsAddEventListenerClose,
		sleepRawWsOnMessage,
		sleepRawWsOnClose,
		sleepRawWsSendOnSleep,
		sleepRawWsDelayedSendOnSleep,
		sleepWithWaitUntilInOnWake,
		sleepAbortListenerVarsActor,
		sleepWaitUntilVarsDuringGrace,
		sleepRawWsVarsExceedsGrace,
		counterWaitUntilProbe,
		// From sleep-db.ts
		sleepWithDb,
		sleepWithSlowScheduledDb,
		sleepWithDbConn,
		sleepWithDbAction,
		sleepWaitUntil,
		sleepNestedWaitUntil,
		sleepEnqueue,
		sleepScheduleAfter,
		sleepOnSleepThrows,
		sleepWaitUntilRejects,
		sleepWaitUntilState,
		sleepWithRawWs,
		sleepWithRawWsCloseDb,
		sleepWithRawWsCloseDbListener,
		sleepWsMessageExceedsGrace,
		sleepWsConcurrentDbExceedsGrace,
		sleepWsActiveDbExceedsGrace,
		sleepKeepAwakeUntilIdle,
		saveStateActor,
		saveStateObserver,
		// From error-handling.ts
		errorHandlingActor,
		customTimeoutActor,
		// From kv.ts
		kvActor,
		// From queue.ts
		queueActor,
		queueLimitedActor,
		manyQueueChildActor,
		manyQueueActionParentActor,
		manyQueueRunParentActor,
		// From action-inputs.ts
		inputActor,
		// From action-timeout.ts
		shortTimeoutActor,
		longTimeoutActor,
		defaultTimeoutActor,
		syncTimeoutActor,
		// From action-types.ts
		syncActionActor,
		asyncActionActor,
		concurrentActionActor,
		nestedActionActor,
		promiseActor,
		// From conn-params.ts
		counterWithParams,
		// From conn-state.ts
		connStateActor,
		// From conn-preflight-visibility.ts
		connPreflightVisibilityActor,
		// From metadata.ts
		metadataActor,
		// From vars.ts
		staticVarActor,
		nestedVarActor,
		dynamicVarActor,
		uniqueVarActor,
		driverCtxActor,
		// From raw-http.ts
		rawHttpActor,
		rawHttpNoHandlerActor,
		rawHttpVoidReturnActor,
		rawHttpHonoActor,
		// From raw-http-request-properties.ts
		rawHttpRequestPropertiesActor,
		// From raw-websocket.ts
		rawWebSocketActor,
		rawWebSocketAsyncOpenActor,
		rawWebSocketBinaryActor,
		rawWebSocketConnContextActor,
		// From reject-connection.ts
		rejectConnectionActor,
		// From request-access.ts
		requestAccessActor,
		// From actor-onstatechange.ts
		onStateChangeActor,
		// From destroy.ts
		destroyActor,
		destroyObserver,
		destroyAbortSignalActor,
		// From hibernation.ts
		hibernationActor,
		hibernationSleepWindowActor,
		// From file-system-hibernation-cleanup.ts
		fileSystemHibernationCleanupActor,
		// From large-payloads.ts
		largePayloadActor,
		largePayloadConnActor,
		// From run.ts
		runWithTicks,
		runWithWakeDeadline,
		runWithQueueConsumer,
		runWithEarlyExit,
		runWithError,
		runSelfInitiatedSleep,
		runSelfInitiatedDestroy,
		runIgnoresAbortStopTimeout,
		runWithoutHandler,
		// From workflow.ts
		workflowCounterActor,
		workflowQueueActor,
		workflowAccessActor,
		workflowCompleteActor,
		workflowDestroyActor,
		workflowFailedStepActor,
		workflowRunningStepActor,
		workflowReplayActor,
		workflowSleepActor,
		workflowStepRollForwardActor,
		workflowTryActor,
		warmupActor,
		workflowStopTeardownActor,
		workflowErrorHookActor,
		workflowErrorHookEffectsActor,
		workflowErrorHookSleepActor,
		workflowNestedLoopActor,
		workflowNestedJoinActor,
		workflowNestedRaceActor,
		workflowSpawnChildActor,
		workflowSpawnParentActor,
		// From actor-db-raw.ts
		dbActorManualWarningsDisabled,
		actorRuntimeSocketWithoutDb,
		dbActorRaw,
		dbActorRuntimeSocketDisabled,
		dbRemoteLifecycleProbe,
		// From actor-db-drizzle-transaction.ts
		dbActorDrizzleMigration,
		dbActorDrizzleMigrationRollback,
		// From db-lifecycle.ts
		dbLifecycle,
		dbLifecycleFailing,
		dbLifecycleObserver,
		// From stateless.ts
		statelessActor,
		// From access-control.ts
		accessControlActor,
		accessControlNoQueuesActor,
		// From start-stop-race.ts
		startStopRaceActor,
		lifecycleObserver,
		// From conn-error-serialization.ts
		connErrorSerializationActor,
		// From db-pragma-migration.ts
		dbPragmaMigrationActor,
		// From db-init-order.ts
		dbInitOrderCreateStateActor,
		dbInitOrderOnCreateActor,
		dbInitOrderCreateVarsActor,
		dbInitOrderOnWakeActor,
		dbInitOrderOnSleepActor,
		dbInitOrderOnDestroyActor,
		dbInitOrderObserver,
		// From state-zod-coercion.ts
		stateZodCoercionActor,
		// From lifecycle-hooks.ts
		beforeConnectTimeoutActor,
		beforeConnectRejectActor,
		beforeConnectGenericErrorActor,
		stateChangeRecursionActor,
		stateChangeReentrantMutationActor,
		...(agentOsTestActor
			? {
					// From agent-os.ts
					agentOsTestActor,
				}
			: {}),
	},
});
