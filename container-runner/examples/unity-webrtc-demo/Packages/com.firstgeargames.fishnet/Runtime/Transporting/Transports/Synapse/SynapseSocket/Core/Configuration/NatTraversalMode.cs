namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Controls which NAT traversal strategy the engine uses when a direct connection fails.
    /// </summary>
    public enum NatTraversalMode
    {
        /// <summary>
        /// NAT traversal is disabled. Only direct connections are attempted.
        /// Use this for servers with a public endpoint.
        /// </summary>
        Disabled,

        /// <summary>
        /// Full-cone UDP hole punching. Both peers must have this mode enabled and must already know
        /// each other's external endpoint (e.g. via the <c>SynapseBeacon</c> rendezvous sister project
        /// or any other signalling channel). Works for full-cone and address-restricted-cone NAT.
        /// </summary>
        FullCone
    }
}
