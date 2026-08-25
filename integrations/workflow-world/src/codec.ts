import { decode, encode } from "cbor-x";

export function encodeValue(value: unknown): Uint8Array | null {
	if (value === undefined) return null;
	return encode(value);
}

export function decodeValue<T = unknown>(value: unknown): T | undefined {
	if (value == null) return undefined;
	if (value instanceof Uint8Array) return decode(value) as T;
	if (Buffer.isBuffer(value)) return decode(value) as T;
	if (value instanceof ArrayBuffer) return decode(new Uint8Array(value)) as T;
	if (Array.isArray(value) && value.every((item) => Number.isInteger(item))) {
		return decode(new Uint8Array(value)) as T;
	}
	if (ArrayBuffer.isView(value)) {
		return decode(
			new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
		) as T;
	}
	return value as T;
}

export function compact<T extends Record<string, unknown>>(obj: T): Partial<T> {
	return Object.fromEntries(
		Object.entries(obj).filter(([, value]) => value !== undefined && value !== null),
	) as Partial<T>;
}
