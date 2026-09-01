using FishNet.Utility.Performance;
using System;

namespace FishNet.Transporting.Synapse
{

    internal struct Packet
    {
        public readonly int ConnectionId;
        public readonly byte[] Data;
        public readonly int Length;
        public readonly byte Channel;

        public Packet(int connectionId, byte[] data, int length, byte channel)
        {
            ConnectionId = connectionId;
            Data = data;
            Length = length;
            Channel = channel;
        }

        public Packet(int connectionId, ArraySegment<byte> segment, byte channel)
        {
            Data = ByteArrayPool.Retrieve(segment.Count);
            Buffer.BlockCopy(segment.Array, segment.Offset, Data, 0, segment.Count);
            ConnectionId = connectionId;
            Length = segment.Count;
            Channel = channel;
        }

        public ArraySegment<byte> GetArraySegment() => new(Data, 0, Length);

        public void Dispose() => ByteArrayPool.Store(Data);
    }

    internal struct RemoteConnectionEvent
    {
        public readonly bool IsConnected;
        public readonly int ConnectionId;

        public RemoteConnectionEvent(bool isConnected, int connectionId)
        {
            IsConnected = isConnected;
            ConnectionId = connectionId;
        }
    }
}
