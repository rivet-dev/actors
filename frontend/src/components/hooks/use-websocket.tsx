import { useCallback, useEffect, useRef, useState } from "react";
import ReconnectingWebSocket from "reconnectingwebsocket";

export type ConnectionStatus =
	| "connecting"
	| "connected"
	| "disconnected"
	| "error";

export const useWebSocket = (
	url: string,
	protocols?: string | string[],
	opts: {
		onMessage?: (event: ReconnectingWebSocket.MessageEvent) => void;
		enabled?: boolean;
	} = { enabled: true },
) => {
	const wsRef = useRef<ReconnectingWebSocket | null>(null);
	const queueRef = useRef<Array<Parameters<WebSocket["send"]>>>([]);
	const [status, setStatus] = useState<ConnectionStatus>("disconnected");
	const [error, setError] = useState<Error | null>(null);

	const onMessageRef = useRef(opts?.onMessage);
	useEffect(() => {
		onMessageRef.current = opts?.onMessage;
	}, [opts?.onMessage]);

	// Centralized connection logic so we can manually reconnect.
	const connect = useCallback(() => {
		const { promise, reject, resolve } = Promise.withResolvers<void>();

		// If there's an existing socket, close it first to avoid duplicate listeners.
		if (wsRef.current) {
			try {
				wsRef.current.close(1000, "unmount");
			} catch (e) {
				console.warn(
					"Error closing existing WebSocket before reconnect",
					e,
				);
				reject(e);
			}
		}

		const finalProtocols = Array.isArray(protocols) 
			? protocols 
			: (protocols ? [protocols] : []);
		if (!finalProtocols.includes("rivet")) {
			finalProtocols.unshift("rivet");
		}

		const ws = new ReconnectingWebSocket(url, finalProtocols, {
			binaryType: "arraybuffer",
		});
		wsRef.current = ws;
		setError(null);

		const onOpen = () => {
			setStatus("connected");
			// Flush queued messages.
			for (const params of queueRef.current) {
				ws.send(...params);
			}
			queueRef.current = [];
			resolve();
		};

		const onClose = () => {
			setStatus("disconnected");
			reject(new Error("WebSocket disconnected"));
		};

		const onError = (event: ReconnectingWebSocket.ErrorEvent) => {
			const error = new Error("WebSocket error occurred", {
				cause: event,
			});
			setStatus("error");
			setError(error);
			reject(error);
		};

		const onMessage = (event: ReconnectingWebSocket.MessageEvent) => {
			onMessageRef.current?.(event);
		};

		const onConnecting = () => {
			setStatus("connecting");
		};

		ws.addEventListener("open", onOpen);
		ws.addEventListener("close", onClose);
		ws.addEventListener("error", onError);
		ws.addEventListener("message", onMessage);
		ws.addEventListener("connecting", onConnecting);

		// Store a cleanup function on the ref for easier manual teardown if needed.
		(ws as any)._cleanup = () => {
			ws.removeEventListener("open", onOpen);
			ws.removeEventListener("close", onClose);
			ws.removeEventListener("error", onError);
			ws.removeEventListener("message", onMessage);
			ws.removeEventListener("connecting", onConnecting);
			try {
				ws.close(1000, "cleanup");
			} catch (e) {
				console.warn("Error closing WebSocket during cleanup", e);
			}
			if (wsRef.current === ws) wsRef.current = null;
		};

		return promise;
	}, [url, protocols]);

	// Initial connect + reconnect when url/protocols change.
	useEffect(() => {
		if (!opts.enabled) return;
		connect();
		return () => {
			if (wsRef.current && (wsRef.current as any)._cleanup) {
				(wsRef.current as any)._cleanup();
			} else if (wsRef.current) {
				try {
					wsRef.current.close(1000, "unmount");
				} catch (e) {
					console.warn("Error closing WebSocket on unmount", e);
				}
				wsRef.current = null;
			}
		};
	}, [connect, opts.enabled]);

	// Manual reconnect function exposed to consumers.
	const reconnect = useCallback(() => {
		if (!opts.enabled) return;
		connect();
	}, [connect, opts.enabled]);

	const sendMessage = useCallback(
		(...params: Parameters<WebSocket["send"]>) => {
			if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) {
				console.warn(
					"WebSocket is not open. Cannot send message.",
					params,
				);
				queueRef.current.push(params);
				return;
			}
			wsRef.current?.send(...params);
		},
		[],
	);

	return { status, error, sendMessage, reconnect };
};
