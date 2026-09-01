namespace SynapseSocket.Security
{

    /// <summary>
    /// Result of running an incoming packet through the lowest-level filter.
    /// </summary>
    public enum FilterResult
    {
        /// <summary>
        /// Packet passed all filter checks and may be processed.
        /// </summary>
        Allowed,

        /// <summary>
        /// Packet size was zero or exceeded the configured maximum.
        /// </summary>
        Oversized,

        /// <summary>
        /// Peer signature is blacklisted.
        /// </summary>
        Blacklisted,

        /// <summary>
        /// Peer exceeded the configured per-second rate limit.
        /// </summary>
        RateLimited,

        /// <summary>
        /// Signature could not be computed or resolved to a non-zero value.
        /// The peer cannot be identified and must be rejected.
        /// Only produced by <see cref="SecurityProvider.InspectNew"/>.
        /// </summary>
        SignatureFailure
    }
}
