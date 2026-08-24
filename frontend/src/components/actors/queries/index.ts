import type { Rivet } from "@rivetkit/engine-api-full";

export type ActorId = string;

export type ActorStatus =
	| "starting"
	| "running"
	| "stopped"
	| "crashed"
	| "sleeping"
	| "pending"
	| "crash-loop"
	| "unknown";

export function getActorStatus(
	actor: Pick<
		Rivet.Actor,
		| "createTs"
		| "destroyTs"
		| "sleepTs"
		| "pendingAllocationTs"
		| "rescheduleTs"
		| "connectableTs"
		| "error"
	>,
): ActorStatus {
	const {
		createTs,
		connectableTs,
		destroyTs,
		sleepTs,
		pendingAllocationTs,
		rescheduleTs,
		error,
	} = actor;

	// Running takes priority over all other statuses.
	if (createTs && connectableTs && !destroyTs) {
		return "running";
	}

	// A destroyed actor with no error is a graceful stop. connectableTs is NOT
	// checked here: the engine clears connectableTs on every destroy (graceful
	// or crash) during deallocation, so a destroyed actor's connectableTs is
	// always null and carries no crash signal. The `error` field does.
	if (createTs && destroyTs && !error) {
		return "stopped";
	}

	// `error` is the authoritative crash signal (the engine sets it only on
	// Error/Lost stops, never on a graceful Ok stop). Takes priority over
	// pending and other non-running statuses.
	if (error) {
		return "crashed";
	}

	if (rescheduleTs) {
		return "crash-loop";
	}

	if (pendingAllocationTs && !connectableTs && !destroyTs) {
		return "pending";
	}

	if (createTs && sleepTs && !destroyTs) {
		return "sleeping";
	}

	if (createTs && !connectableTs && !destroyTs) {
		return "starting";
	}

	return "unknown";
}
