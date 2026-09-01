using System;
using System.Net;
using CodeBoost.CodeAnalysis;

namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Event arguments for <see cref="SynapseManager.PacketSent"/>.
    /// Fires after the send has completed — <see cref="Payload"/> is the original caller-supplied segment.
    /// Do not retain this instance after the handler returns; it is returned to the pool immediately after.
    /// </summary>
    public struct PacketSentEventArgs
    {
        /// <summary>
        /// The remote endpoint the packet was sent to.
        /// </summary>
        public IPEndPoint EndPoint { get; private set; }

        /// <summary>
        /// The original payload segment supplied by the caller.
        /// <see cref="ArraySegment{T}.Count"/> gives the logical byte count.
        /// </summary>
        public ArraySegment<byte> Payload { get; private set; }

        /// <summary>
        /// True if the packet was sent via the reliable channel.
        /// </summary>
        public bool IsReliable { get; private set; }

        /// <summary>
        /// Initialises a new instance of <see cref="PacketSentEventArgs"/>.
        /// </summary>
        public PacketSentEventArgs(IPEndPoint endPoint, ArraySegment<byte> payload, bool isReliable)
        {
            EndPoint = endPoint;
            Payload = payload;
            IsReliable = isReliable;
        }
    }
}
