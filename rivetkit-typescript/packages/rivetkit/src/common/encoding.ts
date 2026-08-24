import type { VersionedDataHandler } from "vbare";
import { z } from "zod/v4";
import { serializeWithEncoding } from "@/serde";
import { assertUnreachable } from "./utils";

/** Data that can be deserialized. */
export type InputData = string | Buffer | Blob | ArrayBufferLike | Uint8Array;

/** Data that's been serialized. */
export type OutputData = string | Uint8Array;

const JSON_COMPAT_BIGINT = "$BigInt";
const JSON_COMPAT_ARRAY_BUFFER = "$ArrayBuffer";
const JSON_COMPAT_UINT8_ARRAY = "$Uint8Array";
const JSON_COMPAT_UNDEFINED = "$Undefined";
const JSON_COMPAT_SET = "$Set";

/**
 * Types that cbor-x encodes natively without any compat transforms.
 */
export type CborSerializable =
	| string
	| number
	| boolean
	| null
	| undefined
	| bigint
	| Date
	| RegExp
	| Error
	| ArrayBuffer
	| Uint8Array
	| Uint8ClampedArray
	| Uint16Array
	| Uint32Array
	| BigUint64Array
	| Int8Array
	| Int16Array
	| Int32Array
	| BigInt64Array
	| Float32Array
	| Float64Array
	| CborSerializable[]
	| Map<CborSerializable, CborSerializable>
	| { [key: string]: CborSerializable };

/**
 * User-facing serializable type. Extends CborSerializable with Set (encoded
 * as a `$Set` tagged array by the JSON compat layer).
 */
export type JsonCompatValue =
	| CborSerializable
	| Set<JsonCompatValue>
	| JsonCompatValue[]
	| Map<JsonCompatValue, JsonCompatValue>
	| { [key: string]: JsonCompatValue };

function isTypedArray(value: unknown): boolean {
	return (
		value instanceof Uint8ClampedArray ||
		value instanceof Uint16Array ||
		value instanceof Uint32Array ||
		value instanceof BigUint64Array ||
		value instanceof Int8Array ||
		value instanceof Int16Array ||
		value instanceof Int32Array ||
		value instanceof BigInt64Array ||
		value instanceof Float32Array ||
		value instanceof Float64Array
	);
}

/**
 * Recursively validates that a value is CBOR serializable. Throws TypeError
 * with a descriptive message for non-serializable values.
 */
export function assertJsonCompatValue(
	value: unknown,
	path = "",
): asserts value is JsonCompatValue {
	if (
		value === null ||
		value === undefined ||
		typeof value === "string" ||
		typeof value === "number" ||
		typeof value === "boolean" ||
		typeof value === "bigint"
	) {
		return;
	}

	if (typeof value === "function") {
		throw new TypeError(
			`Value at ${path || "root"} is a function and is not CBOR serializable`,
		);
	}

	if (typeof value === "symbol") {
		throw new TypeError(
			`Value at ${path || "root"} is a symbol and is not CBOR serializable`,
		);
	}

	if (
		value instanceof Date ||
		value instanceof RegExp ||
		value instanceof Error ||
		value instanceof ArrayBuffer ||
		value instanceof Uint8Array ||
		isTypedArray(value)
	) {
		return;
	}

	if (value instanceof WeakMap) {
		throw new TypeError(
			`Value at ${path || "root"} is a WeakMap and is not CBOR serializable`,
		);
	}

	if (value instanceof WeakSet) {
		throw new TypeError(
			`Value at ${path || "root"} is a WeakSet and is not CBOR serializable`,
		);
	}

	if (value instanceof WeakRef) {
		throw new TypeError(
			`Value at ${path || "root"} is a WeakRef and is not CBOR serializable`,
		);
	}

	if (value instanceof Promise) {
		throw new TypeError(
			`Value at ${path || "root"} is a Promise and is not CBOR serializable`,
		);
	}

	if (value instanceof Map) {
		for (const [k, v] of value.entries()) {
			assertJsonCompatValue(k, `${path || "root"}.key(${String(k)})`);
			assertJsonCompatValue(v, `${path || "root"}.value(${String(k)})`);
		}
		return;
	}

	if (value instanceof Set) {
		let index = 0;
		for (const item of value.values()) {
			assertJsonCompatValue(item, `${path || "root"}.set[${index}]`);
			index++;
		}
		return;
	}

	if (Array.isArray(value)) {
		for (let i = 0; i < value.length; i++) {
			assertJsonCompatValue(value[i], `${path || "root"}[${i}]`);
		}
		return;
	}

	if (isPlainObject(value)) {
		for (const key in value) {
			assertJsonCompatValue(
				value[key as keyof typeof value],
				path ? `${path}.${key}` : key,
			);
		}
		return;
	}

	const typeName =
		typeof value === "object" && value !== null
			? (value.constructor?.name ?? typeof value)
			: typeof value;
	throw new TypeError(
		`Value at ${path || "root"} of type "${typeName}" is not CBOR serializable`,
	);
}

export const EncodingSchema = z.enum(["json", "cbor", "bare"]);

/**
 * Encoding used to communicate between the client & actor.
 */
export type Encoding = z.infer<typeof EncodingSchema>;

/**
 * Helper class that helps serialize data without re-serializing for the same encoding.
 */
export class CachedSerializer<TBare, TJson, T = TBare> {
	#data: T;
	#cache = new Map<Encoding, OutputData>();
	#versionedDataHandler: VersionedDataHandler<TBare>;
	#version: number;
	#zodSchema: z.ZodType<TJson>;
	#toJson: (value: T) => TJson;
	#toBare: (value: T) => TBare;

	constructor(
		data: T,
		versionedDataHandler: VersionedDataHandler<TBare>,
		version: number,
		zodSchema: z.ZodType<TJson>,
		toJson: (value: T) => TJson,
		toBare: (value: T) => TBare,
	) {
		this.#data = data;
		this.#versionedDataHandler = versionedDataHandler;
		this.#version = version;
		this.#zodSchema = zodSchema;
		this.#toJson = toJson;
		this.#toBare = toBare;
	}

	public get rawData(): T {
		return this.#data;
	}

	public serialize(encoding: Encoding): OutputData {
		const cached = this.#cache.get(encoding);
		if (cached) {
			return cached;
		}

		const serialized = serializeWithEncoding(
			encoding,
			this.#data,
			this.#versionedDataHandler,
			this.#version,
			this.#zodSchema,
			this.#toJson,
			this.#toBare,
		);
		this.#cache.set(encoding, serialized);
		return serialized;
	}
}

export async function inputDataToBuffer(
	data: InputData,
): Promise<Uint8Array | string> {
	if (typeof data === "string") {
		return data;
	}
	if (data instanceof Blob) {
		return new Uint8Array(await data.arrayBuffer());
	}
	if (data instanceof Uint8Array) {
		return data;
	}
	if (data instanceof ArrayBuffer || data instanceof SharedArrayBuffer) {
		return new Uint8Array(data);
	}
	throw new Error("Malformed message");
}

function base64EncodeUint8Array(uint8Array: Uint8Array): string {
	let binary = "";
	for (const value of uint8Array) {
		binary += String.fromCharCode(value);
	}
	return btoa(binary);
}

function base64EncodeArrayBuffer(arrayBuffer: ArrayBuffer): string {
	return base64EncodeUint8Array(new Uint8Array(arrayBuffer));
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
	if (value === null || typeof value !== "object") {
		return false;
	}

	const proto = Object.getPrototypeOf(value);
	return proto === Object.prototype || proto === null;
}

export function encodeJsonCompatValue(input: JsonCompatValue): unknown {
	// Primitives
	if (input === null) {
		return input;
	}
	if (input === undefined) {
		return [JSON_COMPAT_UNDEFINED, 0];
	}
	if (
		typeof input === "string" ||
		typeof input === "number" ||
		typeof input === "boolean"
	) {
		return input;
	}
	if (typeof input === "bigint") {
		return [JSON_COMPAT_BIGINT, input.toString()];
	}

	// Binary types with custom encoding
	if (input instanceof ArrayBuffer) {
		return [JSON_COMPAT_ARRAY_BUFFER, base64EncodeArrayBuffer(input)];
	}
	if (input instanceof Uint8Array) {
		return [JSON_COMPAT_UINT8_ARRAY, base64EncodeUint8Array(input)];
	}

	// TypedArrays pass through for cbor-x native handling
	if (isTypedArray(input)) {
		return input;
	}

	// Date, RegExp, and Error pass through for cbor-x native handling
	if (
		input instanceof Date ||
		input instanceof RegExp ||
		input instanceof Error
	) {
		return input;
	}

	// Set uses custom tag encoding
	if (input instanceof Set) {
		const encoded = [...input.values()].map((v) =>
			encodeJsonCompatValue(v as JsonCompatValue),
		);
		return [JSON_COMPAT_SET, encoded];
	}

	// Map recurses into keys and values
	if (input instanceof Map) {
		const encoded = new Map<unknown, unknown>();
		for (const [k, v] of input.entries()) {
			encoded.set(
				encodeJsonCompatValue(k as JsonCompatValue),
				encodeJsonCompatValue(v as JsonCompatValue),
			);
		}
		return encoded;
	}

	// Arrays
	if (Array.isArray(input)) {
		const encoded = input.map((value) =>
			encodeJsonCompatValue(value as JsonCompatValue),
		);
		if (
			encoded.length === 2 &&
			typeof encoded[0] === "string" &&
			(encoded[0] as string).startsWith("$")
		) {
			return [`$${encoded[0]}`, encoded[1]];
		}
		return encoded;
	}

	// Plain objects
	if (isPlainObject(input)) {
		const encoded: Record<string, unknown> = {};
		for (const [key, value] of Object.entries(input)) {
			encoded[key] = encodeJsonCompatValue(value as JsonCompatValue);
		}
		return encoded;
	}

	// Not serializable
	const typeName =
		typeof input === "object" && input !== null
			? ((input as object).constructor?.name ?? typeof input)
			: typeof input;
	throw new TypeError(`Value of type "${typeName}" is not CBOR serializable`);
}

export interface JsonCompatReviveOptions {
	coerceSafeIntegerBigInts?: boolean;
}

export function reviveJsonCompatValue(
	input: any,
	options: JsonCompatReviveOptions = {},
): any {
	if (typeof input === "bigint") {
		if (
			options.coerceSafeIntegerBigInts &&
			input >= BigInt(Number.MIN_SAFE_INTEGER) &&
			input <= BigInt(Number.MAX_SAFE_INTEGER)
		) {
			return Number(input);
		}
		return input;
	}
	if (input instanceof Map) {
		const revived = new Map();
		for (const [k, v] of input.entries()) {
			revived.set(
				reviveJsonCompatValue(k, options),
				reviveJsonCompatValue(v, options),
			);
		}
		return revived;
	}
	if (Array.isArray(input)) {
		if (
			input.length === 2 &&
			typeof input[0] === "string" &&
			input[0].startsWith("$")
		) {
			if (input[0] === JSON_COMPAT_BIGINT) {
				return BigInt(input[1]);
			}
			if (input[0] === JSON_COMPAT_ARRAY_BUFFER) {
				return base64DecodeToArrayBuffer(input[1]);
			}
			if (input[0] === JSON_COMPAT_UINT8_ARRAY) {
				return base64DecodeToUint8Array(input[1]);
			}
			if (input[0] === JSON_COMPAT_UNDEFINED) {
				return undefined;
			}
			if (input[0] === JSON_COMPAT_SET) {
				const items = (input[1] as unknown[]).map((v) =>
					reviveJsonCompatValue(v, options),
				);
				return new Set(items);
			}
			if (input[0].startsWith("$$")) {
				return [
					input[0].substring(1),
					reviveJsonCompatValue(input[1], options),
				];
			}
			throw new Error(
				`Unknown JSON encoding type: ${input[0]}. This may indicate corrupted data or a version mismatch.`,
			);
		}
		return input.map((value) => reviveJsonCompatValue(value, options));
	}
	if (isPlainObject(input)) {
		const decoded: Record<string, unknown> = {};
		for (const [key, value] of Object.entries(input)) {
			decoded[key] = reviveJsonCompatValue(value, options);
		}
		return decoded;
	}
	return input;
}

/** Converts data that was encoded to a string. Some formats do not support raw binary data. */
export function encodeDataToString(message: OutputData): string {
	if (typeof message === "string") {
		return message;
	}
	if (message instanceof Uint8Array) {
		return base64EncodeUint8Array(message);
	}
	assertUnreachable(message);
}

function base64DecodeToUint8Array(base64: string): Uint8Array {
	if (typeof Buffer !== "undefined") {
		return new Uint8Array(Buffer.from(base64, "base64"));
	}

	const binary = atob(base64);
	const bytes = new Uint8Array(binary.length);
	for (let i = 0; i < binary.length; i++) {
		bytes[i] = binary.charCodeAt(i);
	}
	return bytes;
}

function base64DecodeToArrayBuffer(base64: string): ArrayBuffer {
	return base64DecodeToUint8Array(base64).buffer as ArrayBuffer;
}

/** Stringifies with compat for values that BARE & CBOR supports. */
export function jsonStringifyCompat(
	input: any,
	space?: string | number,
): string {
	return JSON.stringify(
		input,
		(_key, value) => {
			if (typeof value === "bigint") {
				return [JSON_COMPAT_BIGINT, value.toString()];
			}
			if (value instanceof ArrayBuffer) {
				return [
					JSON_COMPAT_ARRAY_BUFFER,
					base64EncodeArrayBuffer(value),
				];
			}
			if (value instanceof Uint8Array) {
				return [JSON_COMPAT_UINT8_ARRAY, base64EncodeUint8Array(value)];
			}
			if (
				Array.isArray(value) &&
				value.length === 2 &&
				typeof value[0] === "string" &&
				value[0].startsWith("$")
			) {
				return [`$${value[0]}`, value[1]];
			}
			return value;
		},
		space,
	);
}

/** Parses JSON with compat for values that BARE & CBOR supports. */
export function jsonParseCompat(input: string): any {
	return reviveJsonCompatValue(JSON.parse(input));
}
