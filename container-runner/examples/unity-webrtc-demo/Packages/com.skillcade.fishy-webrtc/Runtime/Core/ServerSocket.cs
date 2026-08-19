#if ENABLE_WEBRTC && (UNITY_STANDALONE || UNITY_SERVER || UNITY_EDITOR)

using FishNet.Managing;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using cakeslice.SimpleWebRTC;

namespace FishNet.Transporting.FishyWebRTC.Server
{
	public class ServerSocket : CommonSocket
	{

		#region Public.
		/// <summary>
		/// Gets the current ConnectionState of a remote client on the server.
		/// </summary>
		/// <param name="connectionId">ConnectionId to get ConnectionState for.</param>
		internal RemoteConnectionState GetConnectionState(int connectionId)
		{
			RemoteConnectionState state = _clients.Contains(connectionId) ? RemoteConnectionState.Started : RemoteConnectionState.Stopped;
			return state;
		}
		#endregion

		#region Private.
		#region Configuration.
		private List<Common.ICEServer> _iceServers;
		/// <summary>
		/// Allowed domain origin.
		/// </summary>
		private string _origin;
		/// <summary>
		/// Whether to suppress CORS headers on the server.
		/// </summary>
		private bool _suppressCorsHeaders;
		/// <summary>
		/// Port used by server.
		/// </summary>
		private ushort _port;
		/// <summary>
		/// Maximum number of allowed clients.
		/// </summary>
		private int _maximumClients;
		/// <summary>
		/// MTU sizes for each channel.
		/// </summary>
		private int _mtu;
		#endregion
		#region Queues.
		/// <summary>
		/// Outbound messages which need to be handled.
		/// </summary>
		private Queue<Packet> _outgoing = new Queue<Packet>();
		/// <summary>
		/// Ids to disconnect next iteration. This ensures data goes through to disconnecting remote connections. This may be removed in a later release.
		/// </summary>
		private List<int> _disconnectingNext = new List<int>();
		/// <summary>
		/// Ids to disconnect immediately.
		/// </summary>
		private List<int> _disconnectingNow = new List<int>();
		/// <summary>
		/// ConnectionEvents which need to be handled.
		/// </summary>
		private Queue<RemoteConnectionEvent> _remoteConnectionEvents = new Queue<RemoteConnectionEvent>();
		#endregion
		/// <summary>
		/// Currently connected clients.
		/// </summary>
		private HashSet<int> _clients = new HashSet<int>();
		/// <summary>
		/// Server socket manager.
		/// </summary>
		private SimpleWebRTCServer _server;
		#endregion

		~ServerSocket()
		{
			StopConnection();
		}

		/// <summary>
		/// Initializes this for use.
		/// </summary>
		/// <param name="t"></param>
		internal void Initialize(Transport t, int unreliableMTU)
		{
			base.Transport = t;
			_mtu = unreliableMTU;
		}

		/// <summary>
		/// Threaded operation to process server actions.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private void Socket()
		{
			_server = new SimpleWebRTCServer(5000, _mtu);

			_server.onConnect += _server_onConnect;
			_server.onDisconnect += _server_onDisconnect;
			_server.onData += _server_onData;
			_server.onError += _server_onError;

			base.SetConnectionState(LocalConnectionState.Starting, true);
			_server.Start(_iceServers, _port, _origin, _suppressCorsHeaders);
			base.SetConnectionState(LocalConnectionState.Started, true);
		}

		/// <summary>
		/// Called when a client connection errors.
		/// </summary>
		private void _server_onError(int clientId, Exception arg2)
		{
			// Log the actual error so we can debug connection issues
			UnityEngine.Debug.LogError($"[FishyWebRTC] Server error for clientId {clientId}: {arg2?.Message ?? "null"}");
			if (arg2 != null)
				UnityEngine.Debug.LogException(arg2);
			
			// Only try to stop if this client was registered
			if (_clients.Contains(clientId))
				StopConnection(clientId, true);
		}

		/// <summary>
		/// Called when receiving data.
		/// </summary>
		private void _server_onData(int clientId, ArraySegment<byte> data)
		{
			if (_server == null || !_server.Active)
				return;

			// Race condition fix: Data arrived before Connect event? Implicitly connect.
			if (!_clients.Contains(clientId))
				_server_onConnect(clientId);

			Channel channel;
			ArraySegment<byte> segment = base.RemoveChannel(data, out channel);

			ServerReceivedDataArgs dataArgs = new ServerReceivedDataArgs(segment, channel, clientId, base.Transport.Index);
			base.Transport.HandleServerReceivedDataArgs(dataArgs);
		}

		/// <summary>
		/// Called when a client connects.
		/// </summary>
		private void _server_onConnect(int clientId)
		{
			if (_server == null || !_server.Active)
				return;

			// Race condition fix: If we already implicitly connected via data, ignore this event.
			if (_clients.Contains(clientId))
				return;

			if (_clients.Count >= _maximumClients)
			{
				_server.KickClient(clientId);
			}
			else
			{
				_clients.Add(clientId);
				base.Transport.HandleRemoteConnectionState(new RemoteConnectionStateArgs(RemoteConnectionState.Started, clientId, base.Transport.Index));
			}
		}

		/// <summary>
		/// Called when a client disconnects.
		/// </summary>
		private void _server_onDisconnect(int clientId)
		{
			StopConnection(clientId, true);
		}

		/// <summary>
		/// Gets the address of a remote connection Id.
		/// </summary>
		/// <param name="connectionId"></param>
		/// <returns>Returns string.empty if Id is not found.</returns>
		internal string GetConnectionAddress(int connectionId)
		{
			if (_server == null || !_server.Active)
				return string.Empty;

			return _server.GetClientAddress(connectionId);
		}

		/// <summary>
		/// Starts the server.
		/// </summary>
		internal bool StartConnection(List<Common.ICEServer> iceServers, ushort port, int maximumClients, string origin, bool suppressCorsHeaders)
		{
			if (base.GetConnectionState() != LocalConnectionState.Stopped)
				return false;

			base.SetConnectionState(LocalConnectionState.Starting, true);

			//Assign properties.
			_port = port;
			_origin = origin;
			_suppressCorsHeaders = suppressCorsHeaders;
			_maximumClients = maximumClients;
			_iceServers = iceServers;
			ResetQueues();
			Socket();
			return true;
		}

		/// <summary>
		/// Stops the local socket.
		/// </summary>
		internal bool StopConnection()
		{
			if (_server == null || base.GetConnectionState() == LocalConnectionState.Stopped || base.GetConnectionState() == LocalConnectionState.Stopping)
				return false;

			ResetQueues();
			base.SetConnectionState(LocalConnectionState.Stopping, true);
			_server.Stop();
			base.SetConnectionState(LocalConnectionState.Stopped, true);

			return true;
		}

		/// <summary>
		/// Stops a remote client disconnecting the client from the server.
		/// </summary>
		/// <param name="connectionId">ConnectionId of the client to disconnect.</param>
		internal bool StopConnection(int connectionId, bool immediately)
		{
			if (_server == null || base.GetConnectionState() != LocalConnectionState.Started)
				return false;

			//Don't disconnect immediately, wait until next command iteration.
			if (!immediately)
			{
				_disconnectingNext.Add(connectionId);
			}
			//Disconnect immediately.
			else
			{
				_server.KickClient(connectionId);
				_clients.Remove(connectionId);
				base.Transport.HandleRemoteConnectionState(new RemoteConnectionStateArgs(RemoteConnectionState.Stopped, connectionId, base.Transport.Index));
			}

			return true;
		}

		/// <summary>
		/// Resets queues.
		/// </summary>
		private void ResetQueues()
		{
			_clients.Clear();
			base.ClearPacketQueue(ref _outgoing);
			_disconnectingNext.Clear();
			_disconnectingNow.Clear();
			_remoteConnectionEvents.Clear();
		}

		/// <summary>
		/// Dequeues and processes commands.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private void DequeueDisconnects()
		{
			int count;

			count = _disconnectingNow.Count;
			//If there are disconnect nows.
			if (count > 0)
			{
				for (int i = 0; i < count; i++)
					StopConnection(_disconnectingNow[i], true);

				_disconnectingNow.Clear();
			}

			count = _disconnectingNext.Count;
			//If there are disconnect next.
			if (count > 0)
			{
				for (int i = 0; i < count; i++)
					_disconnectingNow.Add(_disconnectingNext[i]);

				_disconnectingNext.Clear();
			}
		}

		/// <summary>
		/// Dequeues and processes outgoing.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private void DequeueOutgoing()
		{
			if (base.GetConnectionState() != LocalConnectionState.Started || _server == null)
			{
				//Not started, clear outgoing.
				base.ClearPacketQueue(ref _outgoing);
			}
			else
			{
				int count = _outgoing.Count;
				for (int i = 0; i < count; i++)
				{
					Packet outgoing = _outgoing.Dequeue();
					int connectionId = outgoing.ConnectionId;
					AddChannel(ref outgoing);
					ArraySegment<byte> segment = outgoing.GetArraySegment();

					Common.DeliveryMethod dm = (outgoing.Channel == (byte)Channel.Reliable) ?
						  Common.DeliveryMethod.ReliableOrdered : Common.DeliveryMethod.Unreliable;

					//If over the MTU.
					if (outgoing.Channel == (byte)Channel.Unreliable && segment.Count > _mtu)
					{
						base.Transport.NetworkManager.LogWarning($"Server is sending of {segment.Count} length on the unreliable channel, while the MTU is only {_mtu}. The channel has been changed to reliable for this send.");
						dm = Common.DeliveryMethod.ReliableOrdered;
					}

					//Send to all clients.
					if (connectionId == -1)
						_server.SendAll(_clients, segment, dm);
					//Send to one client.
					else
						_server.SendOne(connectionId, segment, dm);

					outgoing.Dispose();
				}
			}
		}

		/// <summary>
		/// Allows for Outgoing queue to be iterated.
		/// </summary>
		internal void IterateOutgoing()
		{
			if (_server == null)
				return;

			DequeueOutgoing();
			DequeueDisconnects();
		}

		/// <summary>
		/// Iterates the Incoming queue.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		internal void IterateIncoming()
		{
			if (_server == null)
				return;

			//Handle connection and disconnection events.
			//Handle connection and disconnection events.
			// _remoteConnectionEvents are now processed immediately in onConnect, so we don't need to loop here.
			// Kept empty to minimize code shift but the queue logic is removed.

			//Read data from clients.
			_server.ProcessMessageQueue();
		}

		/// <summary>
		/// Sends a packet to a single, or all clients.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		internal void SendToClient(byte channelId, ArraySegment<byte> segment, int connectionId)
		{
			Send(ref _outgoing, channelId, segment, connectionId);
		}

		/// <summary>
		/// Returns the maximum number of clients allowed to connect to the server. If the transport does not support this method the value -1 is returned.
		/// </summary>
		/// <returns></returns>
		internal int GetMaximumClients()
		{
			return _maximumClients;
		}
	}
}

#endif
