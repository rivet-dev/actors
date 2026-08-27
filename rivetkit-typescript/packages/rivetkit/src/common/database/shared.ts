import type { SqliteBindings } from "./config";

/** Migrations may legitimately do substantially more work than request transactions. */
export const MIGRATION_TRANSACTION_TIMEOUT_MS = 5 * 60_000;

export function isManualTransactionControl(query: string): boolean {
	return /^\s*(?:BEGIN|SAVEPOINT|COMMIT|END|ROLLBACK)\b/i.test(query);
}

export function validateTransactionTimeout(timeout: number | undefined): void {
	if (timeout !== undefined && (!Number.isFinite(timeout) || timeout <= 0)) {
		throw new Error(
			"db.transaction() timeout must be a positive finite number of milliseconds",
		);
	}
}

export function validateTransactionName(name: string | undefined): void {
	if (name === undefined) {
		return;
	}
	if (name.length === 0) {
		throw new Error("db.transaction() name must not be empty");
	}
}

type SqliteBindingObject = Record<string, unknown>;

function isSqliteBindingValue(value: unknown): boolean {
	if (
		value === null ||
		typeof value === "number" ||
		typeof value === "string" ||
		typeof value === "bigint" ||
		value instanceof Uint8Array
	) {
		return true;
	}

	if (Array.isArray(value)) {
		return value.every((item) => typeof item === "number");
	}

	return false;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return false;
	}
	return Object.getPrototypeOf(value) === Object.prototype;
}

export function isSqliteBindingObject(
	value: unknown,
): value is SqliteBindingObject {
	if (!isPlainObject(value)) {
		return false;
	}

	return Object.values(value).every((entry) => isSqliteBindingValue(entry));
}

export function isSqliteBindingArray(value: unknown): value is unknown[] {
	return (
		Array.isArray(value) &&
		value.every((entry) => isSqliteBindingValue(entry))
	);
}

export function toSqliteBindings(
	input: unknown[] | SqliteBindingObject,
): SqliteBindings {
	if (Array.isArray(input)) {
		for (const value of input) {
			if (!isSqliteBindingValue(value)) {
				throw new Error(
					`unsupported sqlite binding type: ${typeof value}`,
				);
			}
		}
		return input as SqliteBindings;
	}

	if (isSqliteBindingObject(input)) {
		return input as SqliteBindings;
	}

	throw new Error("unsupported sqlite binding collection");
}

/**
 * Serialize async operations on a shared non-reentrant resource.
 */
export class AsyncMutex {
	#locked = false;
	#waiting: (() => void)[] = [];

	async acquire(): Promise<void> {
		while (this.#locked) {
			await new Promise<void>((resolve) => this.#waiting.push(resolve));
		}
		this.#locked = true;
	}

	release(): void {
		this.#locked = false;
		const next = this.#waiting.shift();
		if (next) {
			next();
		}
	}

	async run<T>(fn: () => Promise<T>): Promise<T> {
		await this.acquire();
		try {
			return await fn();
		} finally {
			this.release();
		}
	}
}
