namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Controls how the engine handles unreliable payloads that exceed the MTU.
    /// </summary>
    public enum UnreliableSegmentMode
    {
        /// <summary>
        /// Throw when an unreliable payload exceeds the MTU.
        /// </summary>
        Disabled,

        /// <summary>
        /// Split into unreliable segments and send immediately.
        /// Segments may arrive out-of-order or be lost entirely.
        /// Incomplete assemblies are evicted after <c>MaximumReliableRetries * ReliableResendMilliseconds</c>.
        /// </summary>
        SegmentUnreliable,

        /// <summary>
        /// Split into reliable segments.
        /// The full message is guaranteed to arrive or the connection is terminated via the reliable retry limit.
        /// </summary>
        SegmentReliable
    }
}
