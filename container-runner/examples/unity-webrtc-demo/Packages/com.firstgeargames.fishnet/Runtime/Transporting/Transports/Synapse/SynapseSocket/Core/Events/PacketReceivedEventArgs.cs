using System;
using SynapseSocket.Connections;

namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Event arguments for <see cref="SynapseManager.PacketReceived"/>.
    /// The <see cref="Payload"/> is backed by a pooled buffer and is only valid for the duration of the handler — copy the data if you need to retain it beyond the callback.
    /// </summary>
    public struct PacketReceivedEventArgs
    {
        /// <summary>
        /// The connection that sent the payload.
        /// </summary>
        public SynapseConnection Connection { get; private set; }

        /// <summary>
        /// The received payload. Backed by a pooled buffer — valid only for the duration of the handler.
        /// Copy if you need to retain the data beyond the callback.
        /// </summary>
        public ArraySegment<byte> Payload { get; private set; }

        /// <summary>
        /// True if the packet was delivered via the reliable channel.
        /// </summary>
        public bool IsReliable { get; private set; }
        
        /// <summary>
        /// Initialises a new instance of <see cref="PacketReceivedEventArgs"/>.
        /// </summary>
        public PacketReceivedEventArgs(SynapseConnection synapseConnection, ArraySegment<byte> payload, bool isReliable)
        {
            Connection = synapseConnection;
            Payload = payload;
            IsReliable = isReliable;
        }        

    }
}
