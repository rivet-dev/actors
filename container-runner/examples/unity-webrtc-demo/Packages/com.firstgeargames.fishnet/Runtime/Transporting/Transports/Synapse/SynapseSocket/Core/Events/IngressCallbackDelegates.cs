using System;
using System.Net;
using SynapseSocket.Connections;
using SynapseSocket.Security;

namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Fired by the ingress path when a payload has been delivered to a connection.
    /// </summary>
    internal delegate void PayloadDeliveredDelegate(SynapseConnection synapseConnection, ArraySegment<byte> payload, bool isReliable);

    /// <summary>
    /// Fired by the ingress path when a connection is established or closed.
    /// </summary>
    internal delegate void ConnectionDelegate(SynapseConnection synapseConnection);

    /// <summary>
    /// Fired by the ingress path when a connection attempt is rejected before it could be established.
    /// </summary>
    internal delegate void ConnectionFailedCallbackDelegate(IPEndPoint? endPoint, ConnectionRejectedReason connectionRejectedReason, string? message);

    /// <summary>
    /// Fired by the ingress path when a violation is detected.
    /// </summary>
    internal delegate void ViolationCallbackDelegate(IPEndPoint endPoint, ulong signature, ViolationReason violationReason, int packetSize, string? details, ViolationAction initialViolationAction);

    /// <summary>
    /// Raised when the ingress path receives a datagram whose first byte is not a recognised
    /// Synapse <see cref="SynapseSocket.Packets.PacketType"/> value, and
    /// <see cref="SynapseSocket.Core.Configuration.SynapseConfig.AllowUnknownPackets"/> is true.
    /// Allows external protocols (e.g. a rendezvous/beacon client) to piggyback on the Synapse UDP
    /// socket so that the NAT mapping opened by talking to the external service is the same mapping
    /// used for P2P traffic.
    /// <para>
    /// The handler must return <see cref="FilterResult.Allowed"/> to accept the packet.
    /// Any other value is treated as a rejection and raises a
    /// <see cref="SynapseSocket.Core.Events.ViolationReason.UnknownPacket"/> violation.
    /// When multiple subscribers are attached, the last subscriber's return value is used.
    /// </para>
    /// <para>
    /// The <paramref name="packet"/> segment references the internal receive buffer and is only
    /// valid for the duration of the callback. Copy any bytes the handler needs to retain.
    /// </para>
    /// </summary>
    /// <param name="fromEndPoint">The source endpoint of the datagram.</param>
    /// <param name="packet">The full raw packet bytes, including the leading type byte.</param>
    /// <returns>
    /// <see cref="FilterResult.Allowed"/> to accept the packet; any other value to reject it and
    /// raise a violation.
    /// </returns>
    public delegate FilterResult UnknownPacketReceivedDelegate(IPEndPoint fromEndPoint, ArraySegment<byte> packet);
}
