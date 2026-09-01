using FishNet.Utility.Performance;
using System;

namespace FishNet.Transporting.FishyWebRTC
{
	internal struct Packet
	{
		public readonly int ConnectionId;
		public byte[] Data;
		public int Length;
		public readonly byte Channel;

		public Packet(int connectionId, byte[] data, int length, byte channel)
		{
			ConnectionId = connectionId;
			Data = data;
			Length = length;
			Channel = channel;
		}

		public Packet(int sender, ArraySegment<byte> segment, byte channel)
		{
			Data = ByteArrayPool.Retrieve(segment.Count);
			Buffer.BlockCopy(segment.Array, segment.Offset, Data, 0, segment.Count);
			ConnectionId = sender;
			Length = segment.Count;
			Channel = channel;
		}

		public ArraySegment<byte> GetArraySegment()
		{
			return new ArraySegment<byte>(Data, 0, Length);
		}

		/// <summary>
		/// Adds on length and resizes Data if needed.
		/// </summary>
		/// <param name="length"></param>
		public void AddLength(int length)
		{
			int totalNeeded = (Length + length);
			if (Data.Length < totalNeeded)
				Array.Resize(ref Data, totalNeeded);

			Length += length;
		}

		public void Dispose()
		{
			ByteArrayPool.Store(Data);
		}
	}
}

namespace FishNet.Transporting.FishyWebRTC.Server
{
	internal struct RemoteConnectionEvent
	{
		public readonly bool Connected;
		public readonly int ConnectionId;
		public RemoteConnectionEvent(bool connected, int connectionId)
		{
			Connected = connected;
			ConnectionId = connectionId;
		}
	}
}

