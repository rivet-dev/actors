using System.Collections.Generic;
using FishNet.Managing.Statistic;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Editing.NetworkProfiler
{

    internal class NetworkTraffic : IResettable
    {
        /// <summary>
        /// PacketGroup for each PacketId processed.
        /// </summary>
        private Dictionary<PacketId, PacketGroup> _packetGroups;
        /// <summary>
        /// Total bytes for all packets.
        /// </summary>
        public ulong Bytes;

        /// <summary>
        /// Adds traffic from a specified packetId.
        /// </summary>
        public void AddPacketIdData(PacketId packetId, string details, ulong bytes, GameObject gameObject) => LAddPacketId(packetId, details, bytes, gameObject);

        /// <summary>
        /// Adds traffic from a specified packetId.
        /// </summary>
        public void AddSocketData(ulong bytes)
        {
            LAddPacketId(NetworkTrafficStatistics.UNSPECIFIED_PACKETID, details: string.Empty, bytes, gameObject: null);
            Bytes += bytes;
        }

        /// <summary>
        /// Adds traffic to a PackerGroup.
        /// </summary>
        private void LAddPacketId(PacketId packetId, string details, ulong bytes, GameObject gameObject)
        {
            if (!_packetGroups.TryGetValue(packetId, out PacketGroup packetGroup))
            {
                packetGroup = ResettableObjectCaches<PacketGroup>.Retrieve();
                packetGroup.Initialize(packetId);

                _packetGroups[packetId] = packetGroup;
            }

            packetGroup.AddPacket(details, bytes, gameObject);
        }

        /// <summary>
        /// Calculates and sets Percentage value on each PacketGroup.
        /// </summary>
        /// <remarks>This should only be called after all PacketGroup entries have been created.</remarks>
        public void SetPacketGroupPercentages()
        {
            //Field would probably get cached at runtime during iteration but let's be certain.
            ulong bytes = Bytes;

            foreach (PacketGroup pg in _packetGroups.Values)
                pg.SetPercent(bytes);
        }

        public void ResetState()
        {
            Bytes = 0;
            ResettableT2CollectionCaches<PacketId, PacketGroup>.StoreAndDefault(ref _packetGroups);
        }

        public void InitializeState()
        {
            _packetGroups = ResettableT2CollectionCaches<PacketId, PacketGroup>.RetrieveDictionary();
        }
    }

    
}