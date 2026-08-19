namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Delegate for <see cref="SynapseManager.PacketReceived"/>.
    /// </summary>
    /// <param name="packetReceivedEventArgs">Details about the received packet including payload and reliability.</param>
    public delegate void PacketReceivedDelegate(PacketReceivedEventArgs packetReceivedEventArgs);

}