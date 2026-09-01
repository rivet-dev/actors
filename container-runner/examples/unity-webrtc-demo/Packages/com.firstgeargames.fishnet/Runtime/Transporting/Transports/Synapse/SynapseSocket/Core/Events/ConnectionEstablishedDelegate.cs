namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Delegate for <see cref="SynapseManager.ConnectionEstablished"/>.
    /// </summary>
    /// <param name="connectionEventArgs">Details about the established connection.</param>
    public delegate void ConnectionEstablishedDelegate(ConnectionEventArgs connectionEventArgs);
}
