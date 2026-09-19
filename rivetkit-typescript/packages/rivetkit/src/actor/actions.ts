import type { PrimitiveSchema } from "./schema";

export type RuntimeActionHandler = (...args: any[]) => any;

export function flattenActionHandlers(
	actions: unknown,
): Record<string, RuntimeActionHandler> {
	const flattened = Object.create(null) as Record<
		string,
		RuntimeActionHandler
	>;
	for (const { name, handler } of collectActionEntries(actions)) {
		flattened[name] = handler;
	}
	return flattened;
}

export function flattenActionInputSchemas(
	actions: unknown,
	schemas: unknown,
): Record<string, PrimitiveSchema> | undefined {
	if (schemas === undefined) return undefined;
	if (!isRecord(schemas)) {
		throw new TypeError("actionInputSchemas must be an object");
	}

	const flattened = Object.create(null) as Record<string, PrimitiveSchema>;
	for (const { name, path } of collectActionEntries(actions)) {
		const nestedSchema = lookupNestedSchema(schemas, path);
		const flatSchema = schemas[name];
		if (
			nestedSchema !== undefined &&
			flatSchema !== undefined &&
			nestedSchema !== flatSchema
		) {
			throw new TypeError(
				`Action input schema \`${name}\` is defined by both a nested path and a dotted key`,
			);
		}

		const schema = nestedSchema ?? flatSchema;
		if (schema !== undefined) {
			flattened[name] = schema as PrimitiveSchema;
		}
	}
	return flattened;
}

/**
 * Flatten a tree of per-action sample rates into dot-separated action names.
 *
 * @throws TypeError when a rate names something that is not an action of
 * `actions`, which is how a JavaScript caller learns about a stale override.
 * @throws RangeError when a rate is not a number from 0 to 1.
 */
export function flattenActionTraceSamplers(
	actions: unknown,
	samplers: unknown,
): Record<string, number> {
	const flattened = Object.create(null) as Record<string, number>;
	if (samplers === undefined) return flattened;
	const handlers = flattenActionHandlers(actions);
	visitTraceSamplers(samplers, [], handlers, flattened);
	return flattened;
}

function visitTraceSamplers(
	value: unknown,
	path: string[],
	handlers: Record<string, RuntimeActionHandler>,
	flattened: Record<string, number>,
): void {
	const location = ["tracing", "actions", ...path].join(".");
	if (typeof value === "number") {
		const name = path.join(".");
		if (!Object.hasOwn(handlers, name)) {
			throw new TypeError(`${location} does not name an action`);
		}
		if (!(value >= 0 && value <= 1)) {
			throw new RangeError(
				`${location} must be from 0 to 1, got ${value}`,
			);
		}
		flattened[name] = value;
		return;
	}
	if (!isRecord(value)) {
		throw new TypeError(
			`${location} must be a sample rate or a group of them`,
		);
	}
	for (const [segment, child] of Object.entries(value)) {
		visitTraceSamplers(child, [...path, segment], handlers, flattened);
	}
}

interface ActionEntry {
	name: string;
	path: string[];
	handler: RuntimeActionHandler;
}

function collectActionEntries(actions: unknown): ActionEntry[] {
	const entries: ActionEntry[] = [];
	const names = new Set<string>();
	visitActionGroup(actions ?? {}, [], entries, names);
	return entries;
}

function visitActionGroup(
	value: unknown,
	path: string[],
	entries: ActionEntry[],
	names: Set<string>,
) {
	if (!isRecord(value)) {
		throw new TypeError(
			`${formatActionPath(path)} must be an action handler or group`,
		);
	}

	for (const [segment, child] of Object.entries(value)) {
		const childPath = [...path, segment];
		if (typeof child === "function") {
			const name = childPath.join(".");
			if (names.has(name)) {
				throw new TypeError(
					`Multiple action definitions flatten to \`${name}\``,
				);
			}
			names.add(name);
			entries.push({
				name,
				path: childPath,
				handler: child as RuntimeActionHandler,
			});
		} else {
			visitActionGroup(child, childPath, entries, names);
		}
	}
}

function lookupNestedSchema(
	schemas: Record<string, unknown>,
	path: string[],
): unknown {
	let value: unknown = schemas;
	for (const segment of path) {
		if (!isRecord(value) || !Object.hasOwn(value, segment)) {
			return undefined;
		}
		value = value[segment];
	}
	return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
	if (typeof value !== "object" || value === null || Array.isArray(value)) {
		return false;
	}
	const prototype = Object.getPrototypeOf(value);
	return prototype === Object.prototype || prototype === null;
}

function formatActionPath(path: string[]): string {
	return path.length === 0 ? "actions" : `Action \`${path.join(".")}\``;
}
