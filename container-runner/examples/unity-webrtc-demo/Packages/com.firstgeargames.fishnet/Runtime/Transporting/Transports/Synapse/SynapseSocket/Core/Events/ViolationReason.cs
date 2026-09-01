namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Reasons a violation was raised on the ingress path.
    /// Some reasons apply only to established connections; others can fire pre-connection.
    /// Surfaced on the <c>ViolationDetected</c> event via <see cref="ViolationEventArgs"/>.
    /// </summary>
    public enum ViolationReason
    {
        /// <summary>
        /// Connection timed out (no traffic received within the configured window).
        /// </summary>
        Timeout,

        /// <summary>
        /// Peer requested disconnection.
        /// </summary>
        PeerDisconnect,

        /// <summary>
        /// Reliable retransmission exhausted the retry budget.
        /// </summary>
        ReliableExhausted,

        /// <summary>
        /// Incoming packet exceeded the configured maximum size.
        /// </summary>
        Oversized,

        /// <summary>
        /// Peer exceeded the per-second packet rate limit.
        /// </summary>
        RateLimitExceeded,

        /// <summary>
        /// Received a malformed or unparseable packet header.
        /// </summary>
        Malformed,

        /// <summary>
        /// Received a datagram whose first byte is not a recognised <see cref="SynapseSocket.Packets.PacketType"/>
        /// and <see cref="SynapseSocket.Core.Configuration.SynapseConfig.AllowUnknownPackets"/> is false,
        /// or the <see cref="SynapseManager.UnknownPacketReceived"/> delegate explicitly rejected the packet.
        /// </summary>
        UnknownPacket
    }
}
