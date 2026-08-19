namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Delegate for <see cref="SynapseManager.ConnectionClosed"/>.
    /// </summary>
    /// <param name="connectionEventArgs">Details about the closed connection.</param>
    public delegate void ConnectionClosedDelegate(ConnectionEventArgs connectionEventArgs);
}
