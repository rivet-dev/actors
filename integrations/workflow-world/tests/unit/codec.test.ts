import { describe, expect, test } from "vitest";
import { compact, decodeValue, encodeValue } from "../../src/codec.ts";

function roundtrip<T>(value: T): T {
	return decodeValue<T>(encodeValue(value)) as T;
}

describe("codec round-trip (large + edge-case payloads)", () => {
	test("undefined encodes to null and decodes back to undefined", () => {
		expect(encodeValue(undefined)).toBeNull();
		expect(decodeValue(null)).toBeUndefined();
		expect(decodeValue(undefined)).toBeUndefined();
	});

	test("primitives and null round-trip", () => {
		for (const value of [0, -1, 3.14159, "", "hello", true, false, null]) {
			expect(roundtrip(value)).toEqual(value);
		}
	});

	test("large string (1 MiB) round-trips", () => {
		const value = "x".repeat(1024 * 1024);
		expect(roundtrip(value)).toBe(value);
	});

	test("deeply nested structure round-trips", () => {
		let nested: unknown = { leaf: 42 };
		for (let i = 0; i < 200; i++) nested = { i, child: nested };
		expect(roundtrip(nested)).toEqual(nested);
	});

	test("all-binary payload (Uint8Array with null bytes) round-trips", () => {
		const bytes = new Uint8Array(2048);
		for (let i = 0; i < bytes.length; i++) bytes[i] = i % 256;
		const output = roundtrip(bytes);
		expect(output).toBeInstanceOf(Uint8Array);
		expect(Array.from(output)).toEqual(Array.from(bytes));
	});

	test("strings containing null bytes and unicode round-trip", () => {
		const value = "a\0b\u{1F680}c\nd\te\\f\"g";
		expect(roundtrip(value)).toBe(value);
	});

	test("mixed nested object with binary, arrays, and large fields round-trips", () => {
		const payload = {
			id: `wrun_${"z".repeat(64)}`,
			input: {
				items: Array.from({ length: 500 }, (_, i) => ({ i, name: `item-${i}` })),
			},
			blob: new Uint8Array([0, 1, 2, 255, 0, 128]),
			meta: {
				nested: { deep: { value: "\0end" } },
				flag: false,
				n: Number.MAX_SAFE_INTEGER,
			},
		};
		expect(roundtrip(payload)).toEqual(payload);
	});

	test("decodeValue accepts Buffer and number-array encodings", () => {
		const encoded = encodeValue({ a: 1, b: "two" });
		expect(encoded).not.toBeNull();
		expect(decodeValue(Buffer.from(encoded!))).toEqual({ a: 1, b: "two" });
		expect(decodeValue(Array.from(encoded!))).toEqual({ a: 1, b: "two" });
	});

	test("compact drops undefined and null but keeps falsy values", () => {
		expect(
			compact({ a: 1, b: undefined, c: null, d: 0, e: "", f: false }),
		).toEqual({ a: 1, d: 0, e: "", f: false });
	});
});
