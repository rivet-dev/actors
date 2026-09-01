#if ENABLE_WEBRTC && (UNITY_STANDALONE || UNITY_WEBGL || UNITY_SERVER || UNITY_EDITOR)

using FishNet.Managing;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using cakeslice.SimpleWebRTC;

namespace FishNet.Transporting.FishyWebRTC.Client
{
	public class ClientSocket : CommonSocket
	{
		~ClientSocket()
		{
			StopConnection();
		}

		#region Private.
		#region Configuration.
		private List<Common.ICEServer> _iceServers;
		/// <summary>
		/// Address to bind server to.
		/// </summary>
		private bool _https = true;
		/// <summary>
		/// Address to bind server to.
		/// </summary>
		private string _address = string.Empty;
		/// <summary>
		/// Port used by server.
		/// </summary>
		private ushort _port;
		/// <summary>
		/// Base path used for HTTP signaling.
		/// </summary>
		private string _path = "/";
		/// <summary>
		/// STUN or TURN address to bind server to.
		/// </summary>
		private string _stunTurnAddress;
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
		#endregion
		/// <summary>
		/// Client socket manager.
		/// </summary>
		private SimpleWebRTCClient _client;
		#endregion

		/// <summary>
		/// Initializes this for use.
		/// </summary>
		/// <param name="t"></param>
		internal void Initialize(Transport t, int mtu)
		{
			base.Transport = t;
			_mtu = mtu;
		}

		/// <summary>
		/// Threaded operation to process client actions.
		/// </summary>
		private void Socket(bool noPort)
		{
			_client = SimpleWebRTCClient.Create(ushort.MaxValue, 5000);

			_client.onConnect += _client_onConnect;
			_client.onDisconnect += _client_onDisconnect;
			_client.onData += _client_onData;
			_client.onError += _client_onError;

			UriBuilder builder = new UriBuilder
			{
				Scheme = _https ? "https" : "http",
				Host = _address,
				Port = noPort ? -1 : _port,
				Path = _path
			};
			base.SetConnectionState(LocalConnectionState.Starting, false);

			_client.Connect(_iceServers, builder.Uri);
		}

		private void _client_onError(Exception obj)
		{
			Log.Exception(obj);
			StopConnection();
		}

		private void _client_onData(ArraySegment<byte> data)
		{
			if (_client == null || _client.ConnectionState != ClientState.Connected)
			{
				return;
			}

			Channel channel;
			data = base.RemoveChannel(data, out channel);
			ClientReceivedDataArgs dataArgs = new ClientReceivedDataArgs(data, channel, base.Transport.Index);
			base.Transport.HandleClientReceivedDataArgs(dataArgs);
		}

		private void _client_onDisconnect()
		{
			StopConnection();
		}

		private void _client_onConnect()
		{
			base.SetConnectionState(LocalConnectionState.Started, false);
		}


		/// <summary>
		/// Starts the client connection.
		/// </summary>
		/// <param name="address"></param>
		/// <param name="port"></param>
		/// <param name="channelsCount"></param>
		/// <param name="pollTime"></param>
		internal bool StartConnection(List<Common.ICEServer> iceServers, string address, bool https, ushort port, bool noPort, string path)
		{
			if (base.GetConnectionState() != LocalConnectionState.Stopped)
				return false;

			base.SetConnectionState(LocalConnectionState.Starting, false);
			//Assign properties.
			_port = port;
			_address = address;
			_https = https;
			_path = path;
			_iceServers = iceServers;

			ResetQueues();
			Socket(noPort);

			return true;
		}


		/// <summary>
		/// Stops the local socket.
		/// </summary>
		internal bool StopConnection()
		{
			if (base.GetConnectionState() == LocalConnectionState.Stopped || base.GetConnectionState() == LocalConnectionState.Stopping)
				return false;

			base.SetConnectionState(LocalConnectionState.Stopping, false);
			_client.Disconnect();
			base.SetConnectionState(LocalConnectionState.Stopped, false);
			return true;
		}

		/// <summary>
		/// Resets queues.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private void ResetQueues()
		{
			base.ClearPacketQueue(ref _outgoing);
		}


		/// <summary>
		/// Dequeues and processes outgoing.
		/// </summary>
		private void DequeueOutgoing()
		{
			int count = _outgoing.Count;
			for (int i = 0; i < count; i++)
			{
				Packet outgoing = _outgoing.Dequeue();
				base.AddChannel(ref outgoing);

				ArraySegment<byte> segment = outgoing.GetArraySegment();
				Common.DeliveryMethod dm = (outgoing.Channel == (byte)Channel.Reliable) ?
								 Common.DeliveryMethod.ReliableOrdered : Common.DeliveryMethod.Unreliable;
				//If over the MTU.
				if (outgoing.Channel == (byte)Channel.Unreliable && segment.Count > _mtu)
				{
					base.Transport.NetworkManager.LogWarning($"Client is sending of {segment.Count} length on the unreliable channel, while the MTU is only {_mtu}. The channel has been changed to reliable for this send.");
					dm = Common.DeliveryMethod.ReliableOrdered;
				}

				_client.Send(segment, dm);
				outgoing.Dispose();
			}
		}

		/// <summary>
		/// Allows for Outgoing queue to be iterated.
		/// </summary>
		internal void IterateOutgoing()
		{
			DequeueOutgoing();
		}

		/// <summary>
		/// Iterates the Incoming queue.
		/// </summary>
		internal void IterateIncoming()
		{
			if (_client == null)
				return;

			/* This has to be called even if not connected because it will also poll events such as
			 * Connected, or Disconnected, ect. */
			_client.ProcessMessageQueue();
		}

		/// <summary>
		/// Sends a packet to the server.
		/// </summary>
		internal void SendToServer(byte channelId, ArraySegment<byte> segment)
		{
			//Not started, cannot send.
			if (base.GetConnectionState() != LocalConnectionState.Started)
				return;

			base.Send(ref _outgoing, channelId, segment, -1);
		}
	}
}

#endif
