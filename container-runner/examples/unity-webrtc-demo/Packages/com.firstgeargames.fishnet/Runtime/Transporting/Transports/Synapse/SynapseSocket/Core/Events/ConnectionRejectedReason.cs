namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Reasons a connection attempt was rejected before (or instead of) being established.
    /// Surfaced on the <c>ConnectionFailed</c> event via <see cref="ConnectionFailedEventArgs"/>.
    /// </summary>
    public enum ConnectionRejectedReason
    {
        /// <summary>
        /// Binding the local socket failed.
        /// </summary>
        BindFailed,

        /// <summary>
        /// Signature validation rejected the peer at handshake.
        /// </summary>
        SignatureRejected,

        /// <summary>
        /// Peer signature is blacklisted.
        /// </summary>
        Blacklisted,

        /// <summary>
        /// Handshake or outbound connect attempt timed out before completing.
        /// </summary>
        Timeout,

        /// <summary>
        /// NAT traversal (hole punching) failed: no response was received after exhausting all punch attempts.
        /// </summary>
        NatTraversalFailed,

        /// <summary>
        /// The engine has reached <see cref="SynapseSocket.Core.Configuration.SynapseConfig.MaximumConcurrentConnections"/>
        /// and cannot accept new peers until an existing connection closes.
        /// </summary>
        ServerFull
    }
}
