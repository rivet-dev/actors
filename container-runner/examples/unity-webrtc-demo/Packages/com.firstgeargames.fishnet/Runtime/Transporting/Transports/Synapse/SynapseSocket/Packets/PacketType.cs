namespace SynapseSocket.Packets
{

    /// <summary>
    /// Identifies the type of a Synapse wire packet.
    /// Encoded as the first byte of every packet header.
    /// Optional header fields follow based on type:
    /// <list type="bullet">
    /// <item><see cref="Reliable"/>, <see cref="Ack"/>, <see cref="ReliableSegmented"/> — sequence number (2 bytes LE).</item>
    /// <item><see cref="Segmented"/> — segment ID (2 bytes LE), segment index (1 byte), segment count (1 byte).</item>
    /// <item><see cref="ReliableSegmented"/> — sequence number, then segment fields.</item>
    /// </list>
    /// All other types carry no additional header fields; any further bytes are payload.
    /// <para>
    /// Byte values strictly greater than <see cref="NatChallenge"/> are reserved for external protocols
    /// that piggyback on the Synapse UDP socket via <c>SynapseManager.UnknownPacketReceived</c>.
    /// </para>
    /// </summary>
    public enum PacketType : byte
    {
        /// <summary>Unreliable, unsegmented data payload.</summary>
        None = 0,

        /// <summary>Reliable, unsegmented data payload. Header includes a sequence number.</summary>
        Reliable = 1,

        /// <summary>Acknowledgment for a reliable packet. Header includes the acknowledged sequence number.</summary>
        Ack = 2,

        /// <summary>Handshake or handshake acknowledgment. Payload contains a 4-byte nonce.</summary>
        Handshake = 3,

        /// <summary>Keep-alive heartbeat. No payload.</summary>
        KeepAlive = 4,

        /// <summary>Graceful disconnect notification. No payload.</summary>
        Disconnect = 5,

        /// <summary>Unreliable, segmented data payload. Header includes segment fields.</summary>
        Segmented = 6,

        /// <summary>Reliable, segmented data payload. Header includes a sequence number then segment fields.</summary>
        ReliableSegmented = 7,

        /// <summary>NAT punch probe. Sent to open a NAT table mapping. No payload.</summary>
        NatProbe = 8,

        /// <summary>NAT challenge or challenge echo. Payload is an 8-byte HMAC token.</summary>
        NatChallenge = 9
    }
}
