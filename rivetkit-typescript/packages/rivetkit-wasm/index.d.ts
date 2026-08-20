export function start(): void;
export function awaitPromise(promise: Promise<any>): Promise<any>;
export function uint8ArrayFromBytes(bytes: Uint8Array): Uint8Array;
export function bridgeRivetErrorPrefix(): string;
export function roundTripBytes(bytes: Uint8Array): Uint8Array;

export class ActorContext {
	free(): void;
	keepAwake(promise: Promise<any>): void;
	saveState(payload: any): Promise<void>;
	saveStateAndWorkflowBatch(
		writes: Array<{ key: Uint8Array; value: Uint8Array }>,
	): Promise<void>;
	waitUntil(promise: Promise<any>): void;
	abortSignal(): any;
	connectConn(params: Uint8Array, request: any): Promise<ConnHandle>;
	requestSave(opts: any): void;
	registerTask(promise: Promise<any>): void;
	runtimeState(): any;
	endKeepAwake(region_id: number): void;
	beginKeepAwake(): number;
	inspectorSnapshot(): object;
	endOnStateChange(): void;
	restartRunHandler(): void;
	beginOnStateChange(): void;
	requestSaveAndWait(opts: any): Promise<void>;
	verifyInspectorAuth(bearer_token?: string | null): Promise<void>;
	endWebsocketCallback(region_id: number): void;
	beginWebsocketCallback(): number;
	dirtyHibernatableConns(): Array<any>;
	kv(): Kv;
	waitForTrackedShutdownWork(): Promise<boolean>;
	takePendingHibernationChanges(): Array<any>;
	key(): any;
	constructor();
	sql(): SqliteDb;
	name(): string;
	conns(): Array<any>;
	queue(): Queue;
	sleep(): void;
	state(): Uint8Array;
	region(): string;
	destroy(): void;
	actorId(): string;
	schedule(): Schedule;
	broadcast(name: string, args: Uint8Array): void;
	setAlarm(timestamp_ms?: number | null): void;
}

export class ActorFactory {
	free(): void;
	constructor(callbacks: any, config: any);
}

export class CancellationToken {
	free(): void;
	onCancelled(callback: Function): void;
	constructor();
	cancel(): void;
	aborted(): boolean;
}

export class ConnHandle {
	private constructor();
	free(): void;
	disconnect(reason?: string | null): Promise<void>;
	isHibernatable(): boolean;
	id(): string;
	send(name: string, args: Uint8Array): void;
	state(): Uint8Array;
	params(): Uint8Array;
	setState(state: Uint8Array): void;
}

export class CoreRegistry {
	free(): void;
	handleServerlessRequest(
		req: any,
		on_stream_event: Function,
		cancel_token: CancellationToken,
		config: any,
	): Promise<any>;
	constructor();
	serve(config: any): Promise<void>;
	register(name: string, factory: ActorFactory): void;
	shutdown(): Promise<void>;
}

export class Kv {
	private constructor();
	free(): void;
	delete(key: Uint8Array): Promise<void>;
	listRange(start: Uint8Array, end: Uint8Array, options: any): Promise<any>;
	listPrefix(prefix: Uint8Array, options: any): Promise<any>;
	batchDelete(keys: Array<any>): Promise<void>;
	deleteRange(start: Uint8Array, end: Uint8Array): Promise<void>;
	get(key: Uint8Array): Promise<any>;
	put(key: Uint8Array, value: Uint8Array): Promise<void>;
	batchGet(keys: Array<any>): Promise<any>;
	batchPut(entries: Array<any>): Promise<void>;
}

export class Queue {
	private constructor();
	free(): void;
	nextBatch(
		options: any,
		signal?: CancellationToken | null,
	): Promise<Array<any>>;
	tryNextBatch(options: any): Array<any>;
	waitForNames(
		names: any,
		options: any,
		signal?: CancellationToken | null,
	): Promise<QueueMessage>;
	enqueueAndWait(
		name: string,
		body: Uint8Array,
		options: any,
		signal?: CancellationToken | null,
	): Promise<Uint8Array | undefined>;
	waitForNamesAvailable(names: any, options: any): Promise<void>;
	inspectMessages(): Promise<Array<any>>;
	completePersisted(
		message_id: bigint,
		expected_name: string,
		response?: Uint8Array | null,
	): Promise<boolean>;
	send(name: string, body: Uint8Array): Promise<QueueMessage>;
	maxSize(): number;
	reset(): Promise<void>;
}

export class QueueMessage {
	private constructor();
	free(): void;
	createdAt(): number;
	isCompletable(): boolean;
	id(): bigint;
	body(): Uint8Array;
	name(): string;
	complete(response: any): Promise<void>;
}

export class Schedule {
	private constructor();
	free(): void;
	after(duration_ms: number, action_name: string, args: Uint8Array): Promise<string>;
	at(timestamp_ms: number, action_name: string, args: Uint8Array): Promise<string>;
	cancel(id: string): Promise<boolean>;
	get(id: string): Promise<any>;
	list(): Promise<any>;
	cronSet(
		name: string,
		expression: string,
		timezone: string | null | undefined,
		action_name: string,
		args: Uint8Array,
		max_history?: number | null,
	): Promise<void>;
	cronEvery(
		name: string,
		interval_ms: number,
		action_name: string,
		args: Uint8Array,
		max_history?: number | null,
	): Promise<void>;
	cronGet(name: string): Promise<any>;
	cronList(): Promise<any>;
	cronDelete(name: string): Promise<boolean>;
	cronHistory(name: string, limit?: number | null): Promise<any>;
}

export class SqliteDb {
	private constructor();
	free(): void;
	run(sql: string, params: any): Promise<any>;
	exec(sql: string): Promise<any>;
	close(): Promise<void>;
	query(sql: string, params: any): Promise<any>;
	execute(sql: string, params: any): Promise<any>;
	executeBatch(statements: Array<any>): Promise<Array<any>>;
	beginTransaction(timeout_ms?: number | null): Promise<SqliteTransaction>;
}

export class SqliteTransaction {
	private constructor();
	free(): void;
	exec(sql: string): Promise<any>;
	execute(sql: string, params: any): Promise<any>;
	commit(): Promise<void>;
	rollback(): Promise<void>;
}

export class WebSocketHandle {
	private constructor();
	free(): void;
	setEventCallback(callback: Function): void;
	send(data: Uint8Array, binary: boolean): void;
	close(code?: number | null, reason?: string | null): Promise<void>;
}

export type InitInput =
	| RequestInfo
	| URL
	| Response
	| BufferSource
	| WebAssembly.Module;
export type SyncInitInput = BufferSource | WebAssembly.Module;
export interface InitOutput {
	readonly memory: WebAssembly.Memory;
}

declare function init(
	module_or_path?: InitInput | Promise<InitInput>,
): Promise<InitOutput>;
export default init;
