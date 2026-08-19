#if !UNITY_WEBGL || UNITY_EDITOR

using System;
using System.Collections.Generic;
using UnityEngine;

namespace cakeslice.SimpleWebRTC
{
	public class SimpleWebRTCServer
	{
		readonly int maxMessagesPerTick;

		public readonly WebRTCServer server;
		readonly BufferPool bufferPool;

		public SimpleWebRTCServer(int maxMessagesPerTick, int maxMessageSize)
		{
			this.maxMessagesPerTick = maxMessagesPerTick;
			bufferPool = new BufferPool(5, 20, maxMessageSize);

			server = new WebRTCServer(maxMessageSize, bufferPool);
		}

		public bool Active { get; private set; }

		public event Action<int> onConnect;
		public event Action<int> onDisconnect;
		public event Action<int, ArraySegment<byte>> onData;
		public event Action<int, Exception> onError;

		public void Start(List<Common.ICEServer> iceServers, ushort port, string origin)
		{
			Start(iceServers, port, origin, false);
		}

		public void Start(List<Common.ICEServer> iceServers, ushort port, string origin, bool suppressCorsHeaders)
		{
			server.Listen(iceServers, port, origin, suppressCorsHeaders);
			Active = true;
		}

		public void Stop()
		{
			server.Stop();
			Active = false;
		}
		public void SendAll(Dictionary<int, short> connections, ArraySegment<byte> source, Common.DeliveryMethod dm)
		{
			ArrayBuffer buffer = bufferPool.Take(source.Count);
			buffer.CopyFrom(source);
			buffer.SetReleasesRequired(connections.Count);

			// make copy of array before for each, data sent to each client is the same
			foreach (short id in connections.Values)
			{
				server.Send(id, buffer, dm);
			}
		}

		public void SendAll(ICollection<int> connectionIds, ArraySegment<byte> source, Common.DeliveryMethod dm)
		{
			ArrayBuffer buffer = bufferPool.Take(source.Count);
			buffer.CopyFrom(source);
			buffer.SetReleasesRequired(connectionIds.Count);

			// make copy of array before for each, data sent to each client is the same
			foreach (int id in connectionIds)
			{
				server.Send(id, buffer, dm);
			}
		}
		public void SendOne(int connectionId, ArraySegment<byte> source, Common.DeliveryMethod dm)
		{
			ArrayBuffer buffer = bufferPool.Take(source.Count);
			buffer.CopyFrom(source);

			server.Send(connectionId, buffer, dm);
		}

		public bool KickClient(int connectionId)
		{
			return server.CloseConnection(connectionId);
		}

		public string GetClientAddress(int connectionId)
		{
			return server.GetClientAddress(connectionId);
		}

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
				 server.receiveQueue.TryDequeue(out Message next)
				 )
			{
				processedCount++;

				switch (next.type)
				{
					case Common.EventType.Connected:
						onConnect?.Invoke(next.connId);
						break;
					case Common.EventType.Data:
						onData?.Invoke(next.connId, next.data.ToSegment());
						next.data.Release();
						break;
					case Common.EventType.Disconnected:
						onDisconnect?.Invoke(next.connId);
						break;
					case Common.EventType.Error:
						onError?.Invoke(next.connId, next.exception);
						break;
				}
			}
		}
	}
}

#endif
