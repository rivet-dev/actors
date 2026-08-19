namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Delegate for <see cref="SynapseManager.PacketSent"/>.
    /// </summary>
    /// <param name="packetSentEventArgs">Details about the sent packet including byte count.</param>
    public delegate void PacketSentDelegate(PacketSentEventArgs packetSentEventArgs);
}
