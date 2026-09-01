namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Actions the engine can take in response to a detected violation.
    /// The default (when no <c>ViolationDetected</c> handler is wired up) is <see cref="KickAndBlacklist"/>.
    /// </summary>
    public enum ViolationAction
    {
        /// <summary>
        /// Take no action at all (packet is NOT dropped). Use sparingly.
        /// </summary>
        Ignore,

        /// <summary>
        /// Drop the offending packet only; leave the connection intact.
        /// </summary>
        Drop,

        /// <summary>
        /// Drop the packet and terminate the connection without blacklisting.
        /// </summary>
        Kick,

        /// <summary>
        /// Drop the packet, terminate the connection, and blacklist the signature. Default.
        /// </summary>
        KickAndBlacklist
    }
}
