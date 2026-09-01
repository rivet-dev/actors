using System;
using System.Collections.Generic;

namespace FishNet.Transporting.FishyWebRTC
{
	public abstract class CommonSocket
	{
		#region Public.
		/// <summary>
		/// Current ConnectionState.
		/// </summary>
		private LocalConnectionState _connectionState = LocalConnectionState.Stopped;
		/// <summary>
		/// Returns the current ConnectionState.
		/// </summary>
		/// <returns></returns>
		internal LocalConnectionState GetConnectionState()
		{
			return _connectionState;
		}
		/// <summary>
		/// Sets a new connection state.
		/// </summary>
		/// <param name="connectionState"></param>
		protected void SetConnectionState(LocalConnectionState connectionState, bool asServer)
		{
			//If state hasn't changed.
			if (connectionState == _connectionState)
				return;

			_connectionState = connectionState;
			if (asServer)
				Transport.HandleServerConnectionState(new ServerConnectionStateArgs(connectionState, Transport.Index));
			else
				Transport.HandleClientConnectionState(new ClientConnectionStateArgs(connectionState, Transport.Index));
		}
		#endregion

		#region Protected.
		/// <summary>
		/// Transport controlling this socket.
		/// </summary>
		protected Transport Transport = null;
		#endregion

		/// <summary>
		/// Sends data to connectionId.
		/// </summary>
		internal void Send(ref Queue<Packet> queue, byte channelId, ArraySegment<byte> segment, int connectionId)
		{
			if (GetConnectionState() != LocalConnectionState.Started)
				return;

			//ConnectionId isn't used from client to server.
			Packet outgoing = new Packet(connectionId, segment, channelId);
			queue.Enqueue(outgoing);
		}

		/// <summary>
		/// Clears a queue using Packet type.
		/// </summary>
		/// <param name="queue"></param>
		internal void ClearPacketQueue(ref Queue<Packet> queue)
		{
			int count = queue.Count;
			for (int i = 0; i < count; i++)
			{
				Packet p = queue.Dequeue();
				p.Dispose();
			}
		}

		/// <summary>
		/// Adds channel to the end of the data.
		/// </summary>
		internal void AddChannel(ref Packet packet)
		{
			int writePosition = packet.Length;
			packet.AddLength(1);
			packet.Data[writePosition] = (byte)packet.Channel;
		}


		/// <summary>
		/// Removes the channel, outputting it and returning a new ArraySegment.
		/// </summary>
		internal ArraySegment<byte> RemoveChannel(ArraySegment<byte> segment, out Channel channel)
		{
			byte[] array = segment.Array;
			int count = segment.Count;

			channel = (Channel)array[count - 1];
			return new ArraySegment<byte>(array, 0, count - 1);
		}

	}

}
