using System;
using System.Collections.Concurrent;
using UnityEngine;
using System.Collections.Generic;

namespace cakeslice.SimpleWebRTC
{
	public enum ClientState
	{
		NotConnected = 0,
		Connecting = 1,
		Connected = 2,
		Disconnecting = 3,
	}
	/// <summary>
	/// Client used to control websockets
	/// <para>Base class used by WebRTCClientWebGL</para>
	/// </summary>
	public abstract class SimpleWebRTCClient
	{
		public static SimpleWebRTCClient Create(int maxMessageSize, int maxMessagesPerTick)
		{
#if UNITY_WEBGL
			return new SimpleWebRTCClientWebGL(maxMessageSize, maxMessagesPerTick);
#else
			return new SimpleWebRTCClientNative(maxMessageSize, maxMessagesPerTick);
#endif
		}

		readonly int maxMessagesPerTick;
		protected readonly int maxMessageSize;
		public readonly ConcurrentQueue<Message> receiveQueue = new ConcurrentQueue<Message>();
		protected readonly BufferPool bufferPool;

		protected ClientState state;

		protected SimpleWebRTCClient(int maxMessageSize, int maxMessagesPerTick)
		{
			this.maxMessageSize = maxMessageSize;
			this.maxMessagesPerTick = maxMessagesPerTick;
			bufferPool = new BufferPool(5, 20, maxMessageSize);
		}

		public ClientState ConnectionState => state;

		public event Action onConnect;
		public event Action onDisconnect;
		public event Action<ArraySegment<byte>> onData;
		public event Action<Exception> onError;

		/// <summary>
		/// Processes all new messages
		/// </summary>
		public void ProcessMessageQueue()
		{
			ProcessMessageQueue(null);
		}

		/// <summary>
		/// Processes all messages while <paramref name="behaviour"/> is enabled
		/// </summary>
		/// <param name="behaviour"></param>
		public void ProcessMessageQueue(MonoBehaviour behaviour)
		{
			int processedCount = 0;
			bool skipEnabled = behaviour == null;
			// check enabled every time incase behaviour was disabled after data
			while (
				 (skipEnabled || behaviour.enabled) &&
				 processedCount < maxMessagesPerTick &&
				 // Dequeue last
				 receiveQueue.TryDequeue(out Message next)
				 )
			{
				processedCount++;

				switch (next.type)
				{
					case Common.EventType.Connected:
						onConnect?.Invoke();
						break;
					case Common.EventType.Data:
						onData?.Invoke(next.data.ToSegment());
						next.data.Release();
						break;
					case Common.EventType.Disconnected:
						onDisconnect?.Invoke();
						break;
					case Common.EventType.Error:
						onError?.Invoke(next.exception);
						break;
				}
			}
		}

		public abstract void Connect(List<Common.ICEServer> iceServers, Uri serverAddress);
		public abstract void Disconnect();
		public abstract void Send(ArraySegment<byte> segment, Common.DeliveryMethod dm);
	}
}
