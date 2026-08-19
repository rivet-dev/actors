namespace SynapseSocket.Connections
{

    /// <summary>
    /// Lifecycle state of a Synapse connection.
    /// </summary>
    public enum ConnectionState
    {
        /// <summary>
        /// Newly created; has not yet completed a handshake.
        /// </summary>
        Pending,

        /// <summary>
        /// Handshake complete; packets may flow in both directions.
        /// </summary>
        Connected,

        /// <summary>
        /// Disconnect requested; cleanup in progress.
        /// </summary>
        Disconnecting,

        /// <summary>
        /// Fully disconnected and eligible for removal.
        /// </summary>
        Disconnected
    }
}
