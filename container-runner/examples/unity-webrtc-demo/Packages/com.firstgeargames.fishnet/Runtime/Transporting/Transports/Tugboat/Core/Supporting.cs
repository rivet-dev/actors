using FishNet.Utility.Performance;
using System;

namespace FishNet.Transporting.Tugboat
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

        public Packet(int sender, ArraySegment<byte> segment, byte channel, int mtu)
        {
            // Prefer to max out returned array to mtu to reduce chance of resizing.
            int arraySize = Math.Max(segment.Count, mtu);
            Data = ByteArrayPool.Retrieve(arraySize);
            Buffer.BlockCopy(segment.Array, segment.Offset, Data, 0, segment.Count);
            ConnectionId = sender;
            Length = segment.Count;
            Channel = channel;
        }

        public ArraySegment<byte> GetArraySegment()
        {
            return new(Data, 0, Length);
        }

        public void Dispose()
        {
            ByteArrayPool.Store(Data);
        }
    }
}

namespace FishNet.Transporting.Tugboat.Server
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