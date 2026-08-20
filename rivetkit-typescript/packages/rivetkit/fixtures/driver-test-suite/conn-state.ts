import onChange from "@rivetkit/on-change";
import { actor } from "rivetkit";

export type ConnState = {
	username: string;
	role: string;
	counter: number;
	createdAt: number;
	noCount: boolean;
	capabilities: { tags: string[] };
};

/**
 * Counts how many write-through proxy layers wrap a value. A value read off a
 * state proxy is wrapped exactly once; more layers mean previously read
 * proxies were persisted back into state.
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

export const connStateActor = actor({
	state: {
		sharedCounter: 0,
		disconnectionCount: 0,
		nested: { tags: ["read", "write"] as string[] },
	},
	// Define connection state
	createConnState: (
		_c,
		params: { username?: string; role?: string; noCount?: boolean },
	): ConnState => {
		return {
			username: params?.username || "anonymous",
			role: params?.role || "user",
			counter: 0,
			createdAt: Date.now(),
			noCount: params?.noCount ?? false,
			capabilities: { tags: ["read", "write"] },
		};
	},
	// Lifecycle hook when a connection is established
	onConnect: (c, conn) => {
		conn.send("connectedFromOnConnect", {
			id: conn.id,
			username: conn.state.username,
		});

		const connFromConns = c.conns.get(conn.id);
		if (!connFromConns) {
			throw new Error("connection missing from c.conns in onConnect");
		}
		connFromConns.send("connectedFromOnConnectConns", {
			id: connFromConns.id,
			username: connFromConns.state.username,
		});

		// Broadcast event about the new connection
		c.broadcast("userConnected", {
			id: conn.id,
			username: "anonymous",
			role: "user",
		});
	},
	// Lifecycle hook when a connection is closed
	onDisconnect: (c, conn) => {
		if (!conn.state?.noCount) {
			c.state.disconnectionCount += 1;
			c.broadcast("userDisconnected", {
				id: conn.id,
			});
		}
	},
	actions: {
		// Action to increment the connection's counter
		incrementConnCounter: (c, amount = 1) => {
			c.conn.state.counter += amount;
		},

		// Action to increment the shared counter
		incrementSharedCounter: (c, amount = 1) => {
			c.state.sharedCounter += amount;
			return c.state.sharedCounter;
		},

		// Get the connection state
		getConnectionState: (c) => {
			return { id: c.conn.id, ...c.conn.state };
		},

		// Check all active connections
		getConnectionIds: (c) => {
			return c.conns
				.entries()
				.filter((c) => !c[1].state?.noCount)
				.map((x) => x[0])
				.toArray();
		},

		// Get disconnection count
		getDisconnectionCount: (c) => {
			return c.state.disconnectionCount;
		},

		// Get all active connection states
		getAllConnectionStates: (c) => {
			return c.conns
				.entries()
				.map(([id, conn]) => ({ id, ...conn.state }))
				.toArray();
		},

		// Send message to a specific connection with matching ID
		sendToConnection: (c, targetId: string, message: string) => {
			if (c.conns.has(targetId)) {
				c.conns
					.get(targetId)
					?.send("directMessage", { from: c.conn.id, message });
				return true;
			} else {
				return false;
			}
		},

		// Update connection state (simulated for tests)
		updateConnection: (
			c,
			updates: Partial<{ username: string; role: string }>,
		) => {
			if (updates.username) c.conn.state.username = updates.username;
			if (updates.role) c.conn.state.role = updates.role;
			return c.conn.state;
		},
		// Replacing state with a spread of the current state is the common
		// update pattern. Each read hands back a deep write-through proxy, so
		// the nested values in the spread are proxies themselves.
		spreadUpdateConnState: (c, iterations: number) => {
			for (let i = 0; i < iterations; i++) {
				c.conn.state = { ...c.conn.state, counter: i };
			}
			return {
				depth: proxyDepth(c.conn.state.capabilities),
				tags: [...c.conn.state.capabilities.tags],
			};
		},

		spreadUpdateActorState: (c, iterations: number) => {
			for (let i = 0; i < iterations; i++) {
				c.state = { ...c.state, sharedCounter: i };
			}
			return {
				depth: proxyDepth(c.state.nested),
				tags: [...c.state.nested.tags],
			};
		},

		disconnectSelf: (c, reason?: string) => {
			c.conn.disconnect(reason ?? "test.disconnect");
			return true;
		},
	},
});
