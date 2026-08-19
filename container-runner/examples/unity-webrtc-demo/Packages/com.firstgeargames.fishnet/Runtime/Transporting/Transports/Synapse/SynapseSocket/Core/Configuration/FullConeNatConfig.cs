namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Settings specific to <see cref="NatTraversalMode.FullCone"/> hole punching.
    /// Both peers must already know each other''s external endpoint before calling <see cref="SynapseSocket.Core.SynapseManager.ConnectAsync"/>.
    /// </summary>
    [System.Serializable]
    public sealed class FullConeNatConfig
    {
        /// <summary>
        /// Milliseconds to wait for a direct handshake to succeed before starting probe bursts.
        /// Allows cheap direct connections to bypass hole-punch overhead entirely.
        /// </summary>
        public uint DirectAttemptMilliseconds = 500;
    }
}
