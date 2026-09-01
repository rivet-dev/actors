using FishNet.Broadcast;
using FishNet.CodeGenerating;
using FishNet.Serializing;
using FishNet.Utility.Performance;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;

namespace FishNet.Managing.Server
{
    public struct ClientConnectionChangeBroadcast : IBroadcast
    {
        public bool Connected;
        public int Id;
    }

    [UseGlobalCustomSerializer]
    public struct ConnectedClientsBroadcast : IBroadcast
    {
        public List<int> Values;
    }

    internal static class ConnectedClientsBroadcastSerializers
    {
        public static void WriteConnectedClientsBroadcast(this Writer writer, ConnectedClientsBroadcast value)
        {
            writer.WriteList(value.Values);
        }

        public static ConnectedClientsBroadcast ReadConnectedClientsBroadcast(this Reader reader)
        {
            List<int> cache = CollectionCaches<int>.RetrieveList();
            reader.ReadList(ref cache);
            return new()
            {
                Values = cache
            };
        }
    }
}