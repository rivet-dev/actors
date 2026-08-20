import onChange from "@rivetkit/on-change";

/**
 * Creates a proxy that tracks deep mutations on an object and calls `commit`
 * after every change. Uses `@rivetkit/on-change` internally, which correctly
 * detects mutations via methods on Map, Set, Date, TypedArrays, and arrays.
 *
 * If the value is not an object (primitive, null, undefined), it is returned
 * as-is since primitives cannot be proxied or mutated.
 *
 * @param value - The root value to watch.
 * @param commit - Called after every detected mutation with the root object.
 * @param beforeChange - Called before every mutation with the new value being
 *   assigned. Throw to reject the change.
 */
export function createWriteThroughProxy<T>(
	value: T,
	commit: (next: T) => void,
	beforeChange?: (newValue: unknown) => void,
): T {
	if (!value || typeof value !== "object") {
		return value;
	}

	return onChange(
		value as T & Record<string, any>,
		() => {
			commit(value);
		},
		{
			// Rejection is throw-based: beforeChange throws to prevent the
			// mutation. We always return true so on-change applies the change
			// if beforeChange did not throw.
			onValidate(_path: string, newValue: unknown) {
				beforeChange?.(newValue);
				return true;
			},
		},
	) as T;
}

/**
 * Returns the raw target behind an `@rivetkit/on-change` proxy, following
 * chains of proxies wrapping proxies until reaching a plain value.
 */
function unwrapProxy(value: unknown): unknown {
	let current = value;
	while (current !== null && typeof current === "object") {
		const target = onChange.target(current as Record<string, any>);
		if (target === current) {
			break;
		}
		current = target;
	}
	return current;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
	const proto = Object.getPrototypeOf(value as object);
	return proto === Object.prototype || proto === null;
}

function unwrapDeep(value: unknown, seen: Set<object>): unknown {
	const unwrapped = unwrapProxy(value);
	if (!unwrapped || typeof unwrapped !== "object") {
		return unwrapped;
	}
	if (seen.has(unwrapped)) {
		return unwrapped;
	}
	seen.add(unwrapped);

	if (Array.isArray(unwrapped)) {
		for (let i = 0; i < unwrapped.length; i++) {
			const child = unwrapDeep(unwrapped[i], seen);
			if (child !== unwrapped[i]) {
				unwrapped[i] = child;
			}
		}
		return unwrapped;
	}

	if (unwrapped instanceof Map) {
		const replacements: [unknown, unknown, unknown][] = [];
		for (const [key, child] of unwrapped.entries()) {
			const nextKey = unwrapDeep(key, seen);
			const nextChild = unwrapDeep(child, seen);
			if (nextKey !== key || nextChild !== child) {
				replacements.push([key, nextKey, nextChild]);
			}
		}
		for (const [key, nextKey, nextChild] of replacements) {
			if (nextKey !== key) {
				unwrapped.delete(key);
			}
			unwrapped.set(nextKey, nextChild);
		}
		return unwrapped;
	}

	if (unwrapped instanceof Set) {
		const replacements: [unknown, unknown][] = [];
		for (const child of unwrapped.values()) {
			const next = unwrapDeep(child, seen);
			if (next !== child) {
				replacements.push([child, next]);
			}
		}
		for (const [child, next] of replacements) {
			unwrapped.delete(child);
			unwrapped.add(next);
		}
		return unwrapped;
	}

	if (isPlainObject(unwrapped)) {
		for (const key of Object.keys(unwrapped)) {
			const child = unwrapDeep(unwrapped[key], seen);
			if (child !== unwrapped[key]) {
				unwrapped[key] = child;
			}
		}
	}

	return unwrapped;
}

/**
 * Strips every `@rivetkit/on-change` proxy out of a value in place, including
 * proxies nested inside plain objects, arrays, `Map`s, and `Set`s.
 *
 * A read of `c.state` or `conn.state` hands back a deep write-through proxy,
 * so an update written as `c.state = { ...c.state, foo }` produces a plain root
 * object whose children are still proxies. Persisting that value as-is makes
 * the next read wrap proxies in another proxy layer, and each layer multiplies
 * the work of traversing the state, so repeated spread updates degrade
 * exponentially. Unwrapping before persisting keeps stored state proxy-free.
 */
export function unwrapWriteThroughProxy<T>(value: T): T {
	return unwrapDeep(value, new Set()) as T;
}
