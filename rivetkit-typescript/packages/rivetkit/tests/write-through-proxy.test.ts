import onChange from "@rivetkit/on-change";
import { describe, expect, test, vi } from "vitest";
import {
	assertJsonCompatValue,
	encodeJsonCompatValue,
	reviveJsonCompatValue,
} from "@/common/encoding";
import {
	createWriteThroughProxy,
	unwrapWriteThroughProxy,
} from "@/registry/write-through-proxy";
import { decodeCborCompat, encodeCborCompat } from "@/serde";

describe("createWriteThroughProxy", () => {
	test("tracks mutations on a record containing all supported types", () => {
		const commit = vi.fn();

		const state = createWriteThroughProxy(
			{
				str: "hello",
				num: 42,
				bool: true,
				nil: null as null,
				undef: undefined as undefined,
				big: 99n,
				date: new Date("2025-01-01"),
				err: new Error("test"),
				buf: new ArrayBuffer(8),
				u8: new Uint8Array([1, 2, 3]),
				u8c: new Uint8ClampedArray([4, 5]),
				u16: new Uint16Array([6]),
				u32: new Uint32Array([7]),
				bu64: new BigUint64Array([8n]),
				i8: new Int8Array([-1]),
				i16: new Int16Array([-2]),
				i32: new Int32Array([-3]),
				bi64: new BigInt64Array([-4n]),
				f32: new Float32Array([1.5]),
				f64: new Float64Array([2.5]),
				arr: [1, "two", 3n],
				map: new Map<string, number>([["a", 1]]),
				set: new Set([10, 20, 30]),
				nested: { inner: { deep: true } },
			},
			commit,
		);

		// Property set on root
		commit.mockClear();
		state.str = "world";
		expect(commit).toHaveBeenCalledTimes(1);
		expect(state.str).toBe("world");

		// Nested property set
		commit.mockClear();
		state.nested.inner.deep = false;
		expect(commit).toHaveBeenCalledTimes(1);
		expect(state.nested.inner.deep).toBe(false);

		// Array mutation via push
		commit.mockClear();
		state.arr.push("four");
		expect(commit).toHaveBeenCalled();
		expect(state.arr).toContain("four");

		// Map.set
		commit.mockClear();
		state.map.set("b", 2);
		expect(commit).toHaveBeenCalled();
		expect(state.map.get("b")).toBe(2);

		// Map.delete
		commit.mockClear();
		state.map.delete("a");
		expect(commit).toHaveBeenCalled();
		expect(state.map.has("a")).toBe(false);

		// Set.add
		commit.mockClear();
		state.set.add(40);
		expect(commit).toHaveBeenCalled();
		expect(state.set.has(40)).toBe(true);

		// Set.delete
		commit.mockClear();
		state.set.delete(10);
		expect(commit).toHaveBeenCalled();
		expect(state.set.has(10)).toBe(false);

		// Date mutation
		commit.mockClear();
		state.date.setFullYear(2030);
		expect(commit).toHaveBeenCalled();
		expect(state.date.getFullYear()).toBe(2030);

		// TypedArray index set
		commit.mockClear();
		state.u8[0] = 99;
		expect(commit).toHaveBeenCalled();
		expect(state.u8[0]).toBe(99);

		// Delete property
		commit.mockClear();
		delete (state as Record<string, unknown>).bool;
		expect(commit).toHaveBeenCalledTimes(1);
	});

	test("returns primitives as-is without proxying", () => {
		const commit = vi.fn();
		expect(createWriteThroughProxy(42, commit)).toBe(42);
		expect(createWriteThroughProxy("hi", commit)).toBe("hi");
		expect(createWriteThroughProxy(null, commit)).toBe(null);
		expect(createWriteThroughProxy(undefined, commit)).toBe(undefined);
	});

	test("beforeChange receives the new value on property set", () => {
		const commit = vi.fn();
		const beforeChange = vi.fn();

		const state = createWriteThroughProxy({ x: 1 }, commit, beforeChange);

		state.x = 99;
		expect(beforeChange).toHaveBeenCalled();
	});

	test("beforeChange rejects non-serializable value through nested path", () => {
		const commit = vi.fn();
		const state = createWriteThroughProxy(
			{ nested: { obj: "ok" as unknown } },
			commit,
			(newValue) => {
				assertJsonCompatValue(newValue);
			},
		);

		expect(() => {
			state.nested.obj = (() => {}) as unknown;
		}).toThrow(TypeError);
		expect(commit).not.toHaveBeenCalled();
		expect(state.nested.obj).toBe("ok");
	});

	test("beforeChange throwing prevents the mutation", () => {
		const commit = vi.fn();
		const state = createWriteThroughProxy({ x: 1 }, commit, () => {
			throw new TypeError("rejected");
		});

		expect(() => {
			state.x = 99;
		}).toThrow("rejected");
		expect(commit).not.toHaveBeenCalled();
		expect(state.x).toBe(1);
	});
});

describe("assertJsonCompatValue", () => {
	test("accepts all supported types without throwing", () => {
		expect(() =>
			assertJsonCompatValue({
				str: "hello",
				num: 42,
				bool: true,
				nil: null,
				undef: undefined,
				big: 99n,
				date: new Date(),
				err: new Error("test"),
				buf: new ArrayBuffer(8),
				u8: new Uint8Array([1]),
				u8c: new Uint8ClampedArray([1]),
				u16: new Uint16Array([1]),
				u32: new Uint32Array([1]),
				bu64: new BigUint64Array([1n]),
				i8: new Int8Array([1]),
				i16: new Int16Array([1]),
				i32: new Int32Array([1]),
				bi64: new BigInt64Array([1n]),
				f32: new Float32Array([1]),
				f64: new Float64Array([1]),
				arr: [1, "two", 3n, [true]],
				map: new Map([["k", "v"]]),
				set: new Set([1, 2]),
				nested: { deep: { value: 42 } },
			}),
		).not.toThrow();
	});

	test("rejects a function", () => {
		expect(() => assertJsonCompatValue(() => {})).toThrow(TypeError);
	});

	test("rejects a nested function", () => {
		expect(() => assertJsonCompatValue({ foo: () => {} })).toThrow(
			TypeError,
		);
	});

	test("rejects a symbol", () => {
		expect(() => assertJsonCompatValue(Symbol("x"))).toThrow(TypeError);
	});

	test("accepts a RegExp", () => {
		expect(() => assertJsonCompatValue(/abc/)).not.toThrow();
	});

	test("rejects a WeakMap", () => {
		expect(() => assertJsonCompatValue(new WeakMap())).toThrow(TypeError);
	});

	test("rejects a WeakSet", () => {
		expect(() => assertJsonCompatValue(new WeakSet())).toThrow(TypeError);
	});

	test("rejects a Promise", () => {
		expect(() => assertJsonCompatValue(Promise.resolve())).toThrow(
			TypeError,
		);
	});

	test("rejects a function inside a Map value", () => {
		expect(() => assertJsonCompatValue(new Map([["k", () => {}]]))).toThrow(
			TypeError,
		);
	});

	test("rejects a function inside a Set", () => {
		expect(() => assertJsonCompatValue(new Set([() => {}]))).toThrow(
			TypeError,
		);
	});

	test("rejects a function inside an array", () => {
		expect(() => assertJsonCompatValue([1, () => {}])).toThrow(TypeError);
	});
});

describe("Set encoding round-trip", () => {
	test("encodeJsonCompatValue encodes Set as $Set tag", () => {
		const encoded = encodeJsonCompatValue(new Set([1, 2, 3]));
		expect(Array.isArray(encoded)).toBe(true);
		expect(encoded[0]).toBe("$Set");
		expect(encoded[1]).toEqual([1, 2, 3]);
	});

	test("reviveJsonCompatValue revives $Set tag to Set", () => {
		const revived = reviveJsonCompatValue(["$Set", [1, 2, 3]]);
		expect(revived).toBeInstanceOf(Set);
		expect(revived).toEqual(new Set([1, 2, 3]));
	});

	test("full CBOR round-trip preserves undefined inside Maps", () => {
		const original = {
			m: new Map<string, unknown>([
				["k", undefined],
				["b", 3n],
			]),
		};
		const encoded = encodeCborCompat(original);
		const decoded = decodeCborCompat<typeof original>(encoded);
		expect(decoded.m).toBeInstanceOf(Map);
		expect(decoded.m.get("k")).toBe(undefined);
		expect(decoded.m.has("k")).toBe(true);
		expect(decoded.m.get("b")).toBe(3n);
	});

	test("full CBOR round-trip preserves Sets", () => {
		const original = { items: new Set([1, "two", 3n]) };
		const encoded = encodeCborCompat(original);
		const decoded = decodeCborCompat<typeof original>(encoded);
		expect(decoded.items).toBeInstanceOf(Set);
		expect(decoded.items.has(1)).toBe(true);
		expect(decoded.items.has("two")).toBe(true);
		expect(decoded.items.has(3n)).toBe(true);
	});
});

describe("encodeJsonCompatValue validation", () => {
	test("throws TypeError for a function value", () => {
		const fn = (() => {}) as never;
		expect(() => encodeJsonCompatValue(fn)).toThrow(TypeError);
	});

	test("throws TypeError for a nested function", () => {
		const obj = { foo: () => {} } as never;
		expect(() => encodeJsonCompatValue(obj)).toThrow(TypeError);
	});
});

/**
 * Counts how many `@rivetkit/on-change` proxy layers wrap a value. A raw value
 * has depth 0.
 */
function proxyDepth(value: unknown): number {
	let depth = 0;
	let current = value;
	while (current !== null && typeof current === "object") {
		const target = onChange.target(current as Record<string, any>);
		if (target === current) {
			break;
		}
		depth++;
		current = target;
	}
	return depth;
}

describe("unwrapWriteThroughProxy", () => {
	test("strips proxies nested in objects, arrays, maps, and sets", () => {
		const raw = {
			nested: { inner: { deep: true } },
			arr: [{ item: 1 }],
			map: new Map<string, { value: number }>([["a", { value: 1 }]]),
			set: new Set([{ value: 2 }]),
		};
		const proxied = createWriteThroughProxy(raw, () => {});

		// Reading through the proxy hands back proxies for every child.
		const spread = {
			nested: proxied.nested,
			arr: proxied.arr,
			map: proxied.map,
			set: proxied.set,
		};
		expect(proxyDepth(spread.nested)).toBe(1);

		const unwrapped = unwrapWriteThroughProxy(spread);
		expect(proxyDepth(unwrapped.nested)).toBe(0);
		expect(proxyDepth(unwrapped.nested.inner)).toBe(0);
		expect(proxyDepth(unwrapped.arr)).toBe(0);
		expect(proxyDepth(unwrapped.arr[0])).toBe(0);
		expect(proxyDepth(unwrapped.map)).toBe(0);
		expect(proxyDepth(unwrapped.map.get("a"))).toBe(0);
		expect(proxyDepth([...unwrapped.set][0])).toBe(0);

		// Unwrapping preserves structure and identity of the raw tree.
		expect(unwrapped.nested).toBe(raw.nested);
		expect(unwrapped.map.get("a")).toEqual({ value: 1 });
		expect([...unwrapped.set]).toEqual([{ value: 2 }]);
	});

	test("returns primitives unchanged and tolerates cycles", () => {
		expect(unwrapWriteThroughProxy(null)).toBe(null);
		expect(unwrapWriteThroughProxy(undefined)).toBe(undefined);
		expect(unwrapWriteThroughProxy(7)).toBe(7);
		expect(unwrapWriteThroughProxy("x")).toBe("x");

		const cyclic: Record<string, unknown> = { name: "root" };
		cyclic.self = cyclic;
		expect(unwrapWriteThroughProxy(cyclic)).toBe(cyclic);
	});

	test("unwraps a value that is itself a proxy", () => {
		const raw = { a: 1 };
		const proxied = createWriteThroughProxy(raw, () => {});
		expect(unwrapWriteThroughProxy(proxied)).toBe(raw);
	});
});

describe("spread state updates", () => {
	// Mirrors how `NativeConnAdapter` and the actor context expose state: the
	// getter hands back a deep write-through proxy and the setter persists the
	// assigned value.
	function createStateHolder(initial: unknown, unwrapOnWrite: boolean) {
		let stored = initial;
		return {
			get raw() {
				return stored;
			},
			get state(): any {
				return createWriteThroughProxy(stored, (next) => {
					stored = next;
				});
			},
			set state(value: unknown) {
				stored = unwrapOnWrite ? unwrapWriteThroughProxy(value) : value;
				// Stand-in for the encode and validate traversal both adapters
				// run on every write.
				assertJsonCompatValue(stored);
				encodeCborCompat(stored);
			},
		};
	}

	const initialState = () => ({
		capability: "write",
		executorCapabilities: {
			tags: ["executor-relay-v1", "filesystem-v2-paths-v1"],
		},
	});

	test("stacks proxy layers on nested values when the raw value is stored", () => {
		const holder = createStateHolder(initialState(), false);

		for (let i = 0; i < 5; i++) {
			holder.state = { ...holder.state, capability: "write" };
		}

		// Each update stores the previous read's proxies, so the next read
		// wraps them again. This nesting is what makes state traversal cost
		// grow exponentially.
		expect(
			proxyDepth((holder.raw as any).executorCapabilities),
		).toBeGreaterThan(1);
	});

	test("keeps stored state proxy-free across repeated spread updates", () => {
		const holder = createStateHolder(initialState(), true);

		const started = performance.now();
		for (let i = 0; i < 12; i++) {
			holder.state = { ...holder.state, capability: "write" };
		}
		const elapsed = performance.now() - started;

		expect(proxyDepth((holder.raw as any).executorCapabilities)).toBe(0);
		expect((holder.raw as any).executorCapabilities.tags).toEqual([
			"executor-relay-v1",
			"filesystem-v2-paths-v1",
		]);
		// Without unwrapping, twelve updates of this tiny state take minutes.
		expect(elapsed).toBeLessThan(1000);
	});
});
