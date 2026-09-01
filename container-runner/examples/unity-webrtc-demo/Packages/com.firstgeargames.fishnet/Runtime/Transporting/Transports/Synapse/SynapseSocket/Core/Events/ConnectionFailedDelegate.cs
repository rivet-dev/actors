namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Delegate for <see cref="SynapseManager.ConnectionFailed"/>.
    /// </summary>
    /// <param name="connectionFailedEventArgs">Details about the rejected connection attempt.</param>
    public delegate void ConnectionFailedDelegate(ConnectionFailedEventArgs connectionFailedEventArgs);
}
