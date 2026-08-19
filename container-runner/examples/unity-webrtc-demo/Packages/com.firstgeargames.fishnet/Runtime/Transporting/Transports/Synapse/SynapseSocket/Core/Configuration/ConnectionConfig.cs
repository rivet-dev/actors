namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Configuration for connection lifecycle: keep-alive heartbeats, timeout detection, and the maintenance sweep window.
    /// </summary>
    [System.Serializable]
    public sealed class ConnectionConfig
    {
        /// <summary>
        /// Interval between keep-alive heartbeats in milliseconds.
        /// </summary>
        public uint KeepAliveIntervalMilliseconds = 5000;

        /// <summary>
        /// Time in milliseconds after which an idle connection is considered timed out.
        /// </summary>
        public uint TimeoutMilliseconds = 15000;

        /// <summary>
        /// Width of the maintenance sweep window in milliseconds.
        /// </summary>
        public uint SweepWindowMilliseconds = 1000;
    }
}
