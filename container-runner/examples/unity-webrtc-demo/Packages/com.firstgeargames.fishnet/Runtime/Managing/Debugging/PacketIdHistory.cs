#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
#if DEVELOPMENT
using FishNet.Managing.Logging;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Transporting;
using System;
using System.Collections.Generic;
using System.Text;
using FishNet.Transporting.Tugboat;

namespace FishNet.Managing.Debugging
{
    internal class PacketIdHistory
    {
        /// <summary>
        /// Last several non-split packetIds to be received on the client.
        /// </summary>
        private readonly Queue<PacketId> _serverPacketsReceived = new();
        /// <summary>
        /// Last several non-split packetIds to be received on the server.
        /// </summary>
        private readonly Queue<PacketId> _clientPacketsReceived = new();
        /// <summary>
        /// StringBuilder to limit garbage allocation.
        /// </summary>
        private static StringBuilder _stringBuilder = new();
        /// <summary>
        /// Maximum number of packets allowed to be queued.
        /// </summary>
        private const int PACKET_COUNT = 5;

        /// <summary>
        /// Resets data.
        /// </summary>
        internal void ResetState(bool packetsFromServer)
        {
            if (packetsFromServer)
                _serverPacketsReceived.Clear();
            else
                _clientPacketsReceived.Clear();
        }

        /// <summary>
        /// Adds a packet to data.
        /// </summary>
        internal void ReceivedPacket(PacketId pId, bool packetFromServer)
        {
            Queue<PacketId> queue = packetFromServer ? _serverPacketsReceived : _clientPacketsReceived;

            queue.Enqueue(pId);

            while (queue.Count > PACKET_COUNT)
                queue.Dequeue();
        }

        /// <summary>
        /// Prints current data.
        /// </summary>
        internal string GetReceivedPacketIds(bool packetsFromServer, bool resetReceived = false)
        {
            string packetOriginTxt = packetsFromServer ? "from Server" : "from Client";

            _stringBuilder.Clear();
            Queue<PacketId> queue = GetQueue(packetsFromServer);

            _stringBuilder.AppendLine($"The last {queue.Count} packets to arrive {packetOriginTxt} are:");
            foreach (PacketId item in queue)
                _stringBuilder.AppendLine($"{item.ToString()}");

            // Attach nob information.
            _stringBuilder.Append($"The last parsed NetworkObject is ");
            NetworkObject lastNob = Reader.LastNetworkObject;
            if (lastNob != null)
                _stringBuilder.Append($"Id {lastNob.ObjectId} on gameObject {lastNob.name}");
            else
                _stringBuilder.Append("Unset");

            // Attach nb information.
            _stringBuilder.Append($", and NetworkBehaviour ");
            NetworkBehaviour lastNb = Reader.LastNetworkBehaviour;
            if (lastNb == null)
                _stringBuilder.Append("Unset");
            else
                _stringBuilder.Append($"{lastNb.GetType().Name}");

            _stringBuilder.Append(".");

            if (resetReceived)
                ResetState(packetsFromServer);

            return _stringBuilder.ToString();
        }

        /// <summary>
        /// Returns which packet queue to use.
        /// </summary>
        private Queue<PacketId> GetQueue(bool packetsFromServer) => packetsFromServer ? _serverPacketsReceived : _clientPacketsReceived;
    }
}
#endif