using System.Collections.Generic;
using FishNet.Managing.Statistic;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Editing.NetworkProfiler
{
    /// <summary>
    /// Container for multiple Packets of the same type.
    /// </summary>
    public class PacketGroup : IResettable
    {
        /// <summary>
        /// PacketId of this metric.
        /// </summary>
        public PacketId PacketId { get; private set; } = PacketId.Unset;
        /// <summary>
        /// Bytes of all packets using PacketId.
        /// </summary>
        public ulong Bytes { get; private set; }
        /// <summary>
        /// Percent Bytes is when compared against Bytes of other PacketMetrics.
        /// </summary>
        /// <remarks>This can only be completed after all Packet entries for each PacketId are added.</remarks>
        public float Percent { get; private set; }
        /// <summary>
        /// True if PacketId is for unspecified packets.
        /// </summary>
        public bool IsUnspecifiedPacketId => PacketId == NetworkTrafficStatistics.UNSPECIFIED_PACKETID;
        /// <summary>
        /// Currently added packets.
        /// </summary>
        private List<Packet> _packets = new();

        public void Initialize(PacketId packetId)
        {
            PacketId = packetId;
        }

        /// <summary>
        /// Adds traffic from a specified packetId.
        /// </summary>
        public void AddPacket(string details, ulong bytes, GameObject gameObject)
        {
            Bytes += bytes;

            _packets.Add(new(details, bytes, gameObject));
        }

        /// <summary>
        /// Sets Percent using Bytes against allPacketGroupBytes.
        /// </summary>
        public void SetPercent(ulong allPacketGroupBytes)
        {
            //Prevent divide by 0.
            if (Bytes == 0)
                Percent = 0;
            else
                Percent = (float)Bytes / allPacketGroupBytes;
        }

        public void ResetState()
        {
            PacketId = PacketId.Unset;
            Bytes = 0;
            Percent = 0f;
            _packets.Clear();
        }

        public void InitializeState() { }
    }
}