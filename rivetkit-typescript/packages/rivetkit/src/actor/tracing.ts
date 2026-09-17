/**
 * Sample rates for individual actions, shaped like the actor's `actions` tree.
 *
 * A key that is not an action is a compile error, so a renamed action cannot
 * leave a rate behind unnoticed. JavaScript callers get the same check when
 * `actor()` runs.
 *
 * @template TActions - The actor's action tree, which supplies the keys.
 */
export type ActionTraceSamplers<TActions> = {
	readonly [K in keyof TActions]?: TActions[K] extends (
		...args: never[]
	) => unknown
		? number
		: ActionTraceSamplers<TActions[K]>;
};

/**
 * Trace sampling for one actor definition.
 *
 * Applies to invocations of this actor type, not to a share of its instances.
 * Without it the process sampler from `OTEL_TRACES_SAMPLER` decides.
 *
 * @template TActions - The actor's action tree.
 */
export interface ActorTracingOptions<TActions> {
	/**
	 * Share of the traces this actor starts that are recorded, from 0 to 1.
	 * `1` records every one, `0.1` about one in ten, chosen by trace ID.
	 *
	 * Only traces that start at the actor are decided here: a client call
	 * without trace context, a raw request, a scheduled action. A call that
	 * arrives inside a caller's trace follows the caller's decision, so `0`
	 * does not remove the actor from a trace its caller recorded.
	 */
	readonly sampler?: number;
	/**
	 * Rates that replace `sampler` for single actions, including scheduled
	 * runs of them. An action's rate is not multiplied with the actor's.
	 */
	readonly actions?: ActionTraceSamplers<TActions>;
}
