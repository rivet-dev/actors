using System;
using CodeBoost.CodeAnalysis;
using CodeBoost.Performance;
using SynapseSocket.Connections;

namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Event arguments for <see cref="SynapseManager.ConnectionEstablished"/> and <see cref="SynapseManager.ConnectionClosed"/>.
    /// </summary>
    public struct ConnectionEventArgs
    {
        /// <summary>
        /// The connection that was established or closed.
        /// </summary>
        public SynapseConnection Connection { get; private set; }

        /// <summary>
        /// Initialises a new instance of <see cref="ConnectionEventArgs"/>.
        /// </summary>
        public ConnectionEventArgs(SynapseConnection synapseConnection)
        {
            Connection = synapseConnection;
        }
    }
}
