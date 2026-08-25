import { actor, event, setup } from "rivetkit";
import type { ActorConn } from "rivetkit/client";
import { createClient, createRivetKit, createRivetKitWithClient } from "./mod";

type Assert<T extends true> = T;
type IsEqual<A, B> =
	(<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2
		? true
		: false;

const counterActor = actor({
	state: {},
	events: {
		updated: event<{ count: number }>(),
		pair: event<[number, string]>(),
	},
	actions: {
		increment: (c, amount: number) => {
			c.broadcast("updated", { count: amount });
			return amount;
		},
	},
});

const registry = setup({
	use: {
		counter: counterActor,
	},
});

const client = createClient<typeof registry>();
const rivet = createRivetKitWithClient(client);
const rivetFromFactory = createRivetKit<typeof registry>();
const actorState = rivet.useActor({
	name: "counter",
	key: ["typecheck"],
});
const actorStateFromFactory = rivetFromFactory.useActor({
	name: "counter",
	key: ["typecheck-factory"],
});

if (actorState.connection) {
	void actorState.connection.increment(1);
}

actorState.useEvent("updated", (payload) => {
	const count: number = payload.count;
	void count;
});

actorState.useEvent("pair", (count, label) => {
	const typedCount: number = count;
	const typedLabel: string = label;
	void typedCount;
	void typedLabel;
});
actorStateFromFactory.useEvent("updated", (payload) => {
	const count: number = payload.count;
	void count;
});

// @ts-expect-error unknown event name should fail
actorState.useEvent("missing", () => {});
// @ts-expect-error callback payload should be typed
actorState.useEvent("updated", (payload: { count: string }) => {
	void payload;
});

type ActualConnection = typeof actorState.connection;
type ExpectedConnection = ActorConn<typeof counterActor> | null;
const connectionTypeCheck: Assert<
	IsEqual<ActualConnection, ExpectedConnection>
> = true;
void connectionTypeCheck;

// An actor may also be identified by its raw id. The typed surface (actions,
// events, params) must match the key-based form.
const idActorState = rivet.useActor({
	name: "counter",
	id: "a1b2c3d4-typecheck",
});

if (idActorState.connection) {
	void idActorState.connection.increment(1);
}

idActorState.useEvent("updated", (payload) => {
	const count: number = payload.count;
	void count;
});

type IdActualConnection = typeof idActorState.connection;
const idConnectionTypeCheck: Assert<
	IsEqual<IdActualConnection, ExpectedConnection>
> = true;
void idConnectionTypeCheck;

// @ts-expect-error must provide either a key or an id
rivet.useActor({ name: "counter" });
