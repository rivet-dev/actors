using System;
using System.Net;
using CodeBoost.CodeAnalysis;
using CodeBoost.Performance;

namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Event arguments for <see cref="SynapseManager.ConnectionFailed"/>.
    /// </summary>
    public struct ConnectionFailedEventArgs
    {
        /// <summary>
        /// The remote endpoint involved in the failure, if known.
        /// </summary>
        public IPEndPoint? EndPoint { get; private set; }

        /// <summary>
        /// The reason the connection was rejected or failed.
        /// </summary>
        public ConnectionRejectedReason Reason { get; private set; }

        /// <summary>
        /// An optional message providing additional context.
        /// </summary>
        public string? Message { get; private set; }

        /// <summary>
        /// Initialises a new instance of <see cref="ConnectionFailedEventArgs"/>.
        /// </summary>
        public ConnectionFailedEventArgs(IPEndPoint? endPoint, ConnectionRejectedReason connectionRejectedReason, string? message)
        {
            EndPoint = endPoint;
            Reason = connectionRejectedReason;
            Message = message;
        }
    }
}
