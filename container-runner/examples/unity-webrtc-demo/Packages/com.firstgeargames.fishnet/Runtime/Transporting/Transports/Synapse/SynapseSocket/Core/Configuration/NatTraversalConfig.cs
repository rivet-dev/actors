namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Configuration for NAT traversal. Assign to <see cref="SynapseConfig.NatTraversal"/>.
    /// <para>
    /// Shared probe settings (<see cref="ProbeCount"/>, <see cref="IntervalMilliseconds"/>, <see cref="MaximumAttempts"/>) apply to <see cref="NatTraversalMode.FullCone"/> once an endpoint is known.
    /// Mode-exclusive settings live in <see cref="FullCone"/>.
    /// </para>
    /// </summary>
    [System.Serializable]
    public sealed class NatTraversalConfig
    {
        /// <summary>
        /// Which NAT traversal strategy to use.
        /// Defaults to <see cref="NatTraversalMode.Disabled"/>; servers with a public IP need no traversal.
        /// </summary>
        public NatTraversalMode Mode = NatTraversalMode.Disabled;

        /// <summary>
        /// Number of probe packets sent per punch attempt.
        /// Each probe is a minimal packet that opens a mapping in the local NAT table without initiating a full handshake.
        /// </summary>
        public uint ProbeCount = 3;

        /// <summary>
        /// Milliseconds between successive hole-punch attempts.
        /// Also controls the minimum interval between outbound probe responses to any single source address.
        /// Setting this very low (e.g. &lt; 50 ms) widens the UDP amplification window: a spoofed-source probe forces an outbound probe + handshake at up to <c>1000 / IntervalMilliseconds</c> packets per second per source IP.
        /// The default of 200 ms caps the response rate at 5 pps per address.
        /// </summary>
        public uint IntervalMilliseconds = 200;

        /// <summary>
        /// Maximum number of hole-punch attempts before the connection is declared failed.
        /// </summary>
        public uint MaximumAttempts = 10;

        /// <summary>
        /// Settings exclusive to <see cref="NatTraversalMode.FullCone"/> direct hole-punching.
        /// </summary>
        public FullConeNatConfig FullCone = new();
    }
}
