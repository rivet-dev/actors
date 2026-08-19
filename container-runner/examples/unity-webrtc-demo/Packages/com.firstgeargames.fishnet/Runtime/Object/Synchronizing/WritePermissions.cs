namespace FishNet.Object.Synchronizing
{
    /// <summary>
    /// Which clients or server may write updates.
    /// </summary>
    public enum WritePermission : byte
    {
        /// <summary>
        /// Only the server can change the value of the SyncType.
        /// </summary>
        ServerOnly = 0,
        /// <summary>
        /// Server and clients can change the value of the SyncType. When changed by client the value is not sent to the server.
        /// </summary>
        ClientUnsynchronized = 1
    }
}