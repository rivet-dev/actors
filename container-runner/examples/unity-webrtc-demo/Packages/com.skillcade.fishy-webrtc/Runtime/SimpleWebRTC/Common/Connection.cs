#if !UNITY_WEBGL || UNITY_EDITOR

using System;
using System.Collections.Generic;
using System.Collections.Concurrent;
using System.Threading;
using Unity.WebRTC;
using System.Net;
using System.IO;
using ConnectionId = System.UInt16;

namespace cakeslice.SimpleWebRTC
{
	internal sealed class Connection : IDisposable
	{
		public List<RTCIceCandidate> iceCandidates = new List<RTCIceCandidate>();
		public HttpListenerResponse iceCandidateResponse;

		public RTCDataChannel reliableDataChannel;
		public RTCDataChannel unreliableDataChannel;

		public Config receiveConfig;

		public string clientAddress;

		//

		public RTCPeerConnection client;
		public ConnectionId connId;

		public Thread unreliableSendThread;
		public Thread reliableSendThread;

		public ManualResetEventSlim unreliableSendPending = new ManualResetEventSlim(false);
		public ConcurrentQueue<ArrayBuffer> unreliableSendQueue = new ConcurrentQueue<ArrayBuffer>();
		public ManualResetEventSlim reliableSendPending = new ManualResetEventSlim(false);
		public ConcurrentQueue<ArrayBuffer> reliableSendQueue = new ConcurrentQueue<ArrayBuffer>();

		readonly object disposedLock = new object();
		public Action<Connection> onDispose;
		volatile bool hasDisposed;

		public Connection(List<Common.ICEServer> _iceServers, ConnectionId connId, int maxMessageSize, string clientAddress, Action<Connection> onDispose)
		{
			this.connId = connId;

			this.onDispose = onDispose;

			this.clientAddress = clientAddress;

			client = new RTCPeerConnection();

			List<RTCIceServer> iceServers = new List<RTCIceServer>();
			foreach (Common.ICEServer s in _iceServers)
			{
				if (s.username == null || s.username == "")
					iceServers.Add(new RTCIceServer
					{
						urls = new[] { s.url }
					});
				else
					iceServers.Add(new RTCIceServer
					{
						urls = new[] { s.url },
						username = s.username,
						credential = s.credential,
						credentialType = RTCIceCredentialType.Password
					});
			}

			var configuration = new RTCConfiguration
			{
				iceServers = iceServers.ToArray()
			};
			var error = client.SetConfiguration(ref configuration);
			if (error != RTCErrorType.None)
			{
				Log.Error("RTCError in client.SetConfiguration");
			}

			client.OnConnectionStateChange += state =>
			{
				UnityEngine.Debug.Log($"[webrtc] server peer {connId} connection: {state}");
				if (state == RTCPeerConnectionState.Connected)
				{
					Thread u = new Thread(() =>
					{
						SendLoop.Config sendConfig = new SendLoop.Config(
									this, bufferSize: maxMessageSize, Common.DeliveryMethod.Unreliable);
						SendLoop.Loop(sendConfig);
					});
					unreliableSendThread = u;
					unreliableSendThread.IsBackground = true;
					unreliableSendThread.Name = $"UnreliableSendLoop {connId}";
					unreliableSendThread.Start();

					Thread r = new Thread(() =>
					{
						SendLoop.Config sendConfig = new SendLoop.Config(
									this, bufferSize: maxMessageSize, Common.DeliveryMethod.ReliableOrdered);
						SendLoop.Loop(sendConfig);
					});
					reliableSendThread = r;
					reliableSendThread.IsBackground = true;
					reliableSendThread.Name = $"ReliableSendLoop {connId}";
					reliableSendThread.Start();
				}
			};

			//

			client.OnIceConnectionChange += state =>
			{
				UnityEngine.Debug.Log($"[webrtc] server peer {connId} ICE connection: {state}");
				if (state == RTCIceConnectionState.Disconnected)
					Dispose();
			};
			client.OnIceGatheringStateChange += state =>
				UnityEngine.Debug.Log($"[webrtc] server peer {connId} ICE gathering: {state}");
			client.OnIceCandidate += candidate =>
			{
				if (candidate == null)
					return;
				if (candidate.Candidate == "")
					return;
				if (candidate.Protocol != RTCIceProtocol.Udp)
					return;

				lock (iceCandidates)
				{
					iceCandidates.Add(candidate);
				}
				UnityEngine.Debug.Log(
					$"[webrtc] server peer {connId} ICE candidate: type={candidate.Type} " +
					$"protocol={candidate.Protocol} address={candidate.Address}:{candidate.Port} " +
					$"related={candidate.RelatedAddress}:{candidate.RelatedPort}");
			};

			this.unreliableDataChannel = client.CreateDataChannel("Unreliable", new RTCDataChannelInit()
			{
				ordered = false,
				maxRetransmits = 0,
				//maxPacketLifeTime = 100  // mutually exclusive to RTCDataChannelInit spec!
			});
			this.reliableDataChannel = client.CreateDataChannel("Reliable", new RTCDataChannelInit()
			{
				ordered = true
			});

			this.unreliableDataChannel.OnClose += () =>
			{
				Dispose();
			};
			this.reliableDataChannel.OnClose += () =>
			{
				Dispose();
			};

			this.unreliableDataChannel.OnMessage += bytes => { HandleUnreliableReceiveMessage(bytes); };
			this.reliableDataChannel.OnMessage += bytes => { HandleReliableReceiveMessage(bytes); };
		}

		/// <summary>
		/// disposes client and stops threads
		/// </summary>
		public void Dispose()
		{
			Log.Verbose($"Dispose {ToString()}");

			// check hasDisposed first to stop ThreadInterruptedException on lock
			if (hasDisposed) { return; }

			Log.Info($"Connection Close: {ToString()}");

			lock (disposedLock)
			{
				// check hasDisposed again inside lock to make sure no other object has called this
				if (hasDisposed) { return; }
				hasDisposed = true;

				// stop threads first so they dont try to use disposed objects
				unreliableSendThread?.Interrupt();
				reliableSendThread?.Interrupt();

				try
				{
					// stream 
					client?.Dispose();
					client = null;
					reliableDataChannel?.Dispose();
					reliableDataChannel = null;
					unreliableDataChannel?.Dispose();
					unreliableDataChannel = null;
				}
				catch (Exception e)
				{
					Log.Exception(e);
				}

				unreliableSendPending.Dispose();
				reliableSendPending.Dispose();

				// release all buffers in send queue
				while (unreliableSendQueue.TryDequeue(out ArrayBuffer buffer))
				{
					buffer.Release();
				}
				while (reliableSendQueue.TryDequeue(out ArrayBuffer buffer))
				{
					buffer.Release();
				}

				onDispose.Invoke(this);
			}
		}

		void HandleUnreliableReceiveMessage(byte[] bytes)
		{
			HandleReceiveMessage(bytes, Common.DeliveryMethod.Unreliable);
		}
		void HandleReliableReceiveMessage(byte[] bytes)
		{
			HandleReceiveMessage(bytes, Common.DeliveryMethod.ReliableOrdered);
		}

		void HandleReceiveMessage(byte[] bytes, Common.DeliveryMethod dm)
		{
			(Connection conn, int _, ConcurrentQueue<Message> queue, BufferPool _) = receiveConfig;

			RTCPeerConnection client = conn.client;

			bool dispose = false;
			// TODO: Maybe we shouldn't close/dispose in all cases, only when actually disconnecting...

			try
			{
				ReadOneMessage(receiveConfig, bytes, dm);
			}
			catch (ObjectDisposedException e)
			{
				dispose = true;
				Log.InfoException(e);
			}
			catch (IOException e)
			{
				dispose = true;
				// this could happen if client disconnects
				Log.Warn($"HandleReceiveMessage IOException\n{e.Message}");
				queue.Enqueue(new Message(conn.connId, e));
			}
			catch (InvalidDataException e)
			{
				dispose = true;
				Log.Warn($"Invalid data from {conn}: {e.Message}");
				queue.Enqueue(new Message(conn.connId, e));
			}
			catch (Exception e)
			{
				dispose = true;
				Log.Exception(e);
				queue.Enqueue(new Message(conn.connId, e));
			}

			if (dispose)
			{
				Log.Warn("Closing connection due to exception");
				conn.Dispose();
			}
		}

		//

		public struct Config
		{
			public readonly Connection conn;
			public readonly int maxMessageSize;
			public readonly ConcurrentQueue<Message> queue;
			public readonly BufferPool bufferPool;

			public Config(Connection conn, int maxMessageSize, ConcurrentQueue<Message> queue, BufferPool bufferPool)
			{
				this.conn = conn ?? throw new ArgumentNullException(nameof(conn));
				this.maxMessageSize = maxMessageSize;
				this.queue = queue ?? throw new ArgumentNullException(nameof(queue));
				this.bufferPool = bufferPool ?? throw new ArgumentNullException(nameof(bufferPool));
			}

			public void Deconstruct(out Connection conn, out int maxMessageSize, out ConcurrentQueue<Message> queue, out BufferPool bufferPool)
			{
				conn = this.conn;
				maxMessageSize = this.maxMessageSize;
				queue = this.queue;
				bufferPool = this.bufferPool;
			}
		}

		static void ReadOneMessage(Config config, byte[] bytes, Common.DeliveryMethod dm)
		{
			(Connection conn, int maxMessageSize, ConcurrentQueue<Message> _, BufferPool _) = config;

			var stream = dm == Common.DeliveryMethod.ReliableOrdered ? conn.reliableDataChannel : conn.unreliableDataChannel;

			int msgLength = bytes.Length;
			if (msgLength > maxMessageSize)
				throw new InvalidDataException("Message length is greater than max length");

			HandleMessage(config, bytes, 0, bytes.Length);
		}

		static void HandleMessage(Config config, byte[] bytes, int msgOffset, int payloadLength)
		{
			(Connection conn, int _, ConcurrentQueue<Message> queue, BufferPool bufferPool) = config;

			ArrayBuffer arrayBuffer = bufferPool.Take(payloadLength);

			arrayBuffer.CopyFrom(bytes, msgOffset, payloadLength);

			Log.DumpBuffer($"Message", bytes, msgOffset, payloadLength);

			queue.Enqueue(new Message(conn.connId, arrayBuffer));
		}
	}
}

#endif
