using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Net;
using CodeBoost.Performance;
using SynapseSocket.Core;
using SynapseSocket.Core.Events;

namespace SynapseSocket.Connections
{

    /// <summary>
    /// Handles the lifecycle and state of active sessions.
    /// Thread-safe.
    /// </summary>
    public sealed class ConnectionManager
    {
        /// <summary>
        /// Current live connection count.
        /// </summary>
        public int Count => _connectionsByEndPoint.Count;
        /// <summary>
        /// Raised when two connections produce the same 64-bit signature (birthday-bound collision).
        /// The newer connection wins the reverse-lookup slot. Subscribe for telemetry; no corrective action is taken automatically.
        /// </summary>
        public event SignatureCollisionDelegate? SignatureCollisionDetected;
        /// <summary>
        /// Maps a connection's IPEndPoint to its <see cref="SynapseConnection"/>.
        /// </summary>
        public IReadOnlyDictionary<IPEndPoint, SynapseConnection> ConnectionsByEndPoint => _connectionsByEndPoint;
        private readonly ConcurrentDictionary<IPEndPoint, SynapseConnection> _connectionsByEndPoint = new(IPEndPointComparer.Default);
        /// <summary>
        /// Maps a connection's 64-bit signature to its <see cref="SynapseConnection"/>.
        /// </summary>
        public IReadOnlyDictionary<ulong, SynapseConnection> ConnectionsBySignature => _connectionsBySignature;
        private readonly ConcurrentDictionary<ulong, SynapseConnection> _connectionsBySignature = new();
        /// <summary>
        /// Connections as an index-based collection.
        /// </summary>
        public IReadOnlyList<SynapseConnection> Connections => _connections;
        private readonly List<SynapseConnection> _connections = new();


        /// <summary>
        /// Registers a new connection.
        /// Returns the existing one if already present.
        /// </summary>
        /// <param name="endPoint">The remote endpoint identifying the peer.</param>
        /// <param name="signature">The 64-bit signature associated with the peer.</param>
        /// <param name="isFound"></param>
        /// <returns>The existing connection for the endpoint, or the newly created one.</returns>
        public SynapseConnection GetOrAdd(IPEndPoint endPoint, ulong signature, out bool isFound)
        {
            isFound = _connectionsByEndPoint.TryGetValue(endPoint, out SynapseConnection? synapseConnection);

            if (!isFound)
            {
                synapseConnection = ResettableObjectPool<SynapseConnection>.Rent();

                int connectionsIndex = _connections.Count;
                synapseConnection.Initialize(endPoint, signature, connectionsIndex);

                _connectionsByEndPoint[endPoint] = synapseConnection;
                _connections.Add(synapseConnection);
            }

            if (!_connectionsBySignature.TryAdd(signature, synapseConnection))
            {
                // Two distinct endpoints produced the same 64-bit signature.
                // Overwrite so reverse lookup stays current, but surface the collision.
                _connectionsBySignature[signature] = synapseConnection;
                SignatureCollisionDetected?.Invoke(signature);
            }

            return synapseConnection!;
        }

        /// <summary>
        /// Always creates a fresh <see cref="SynapseConnection"/> for <paramref name="endPoint"/>,
        /// removing any existing entry first. Used by the outbound connect path to guarantee a clean
        /// connection object on every call regardless of prior session state.
        /// </summary>
        /// <param name="endPoint">The remote endpoint to connect to.</param>
        /// <param name="signature">The 64-bit signature associated with the peer.</param>
        /// <returns>The newly created <see cref="SynapseConnection"/>.</returns>
        public SynapseConnection CreateNew(IPEndPoint endPoint, ulong signature)
        {
            if (_connectionsByEndPoint.TryRemove(endPoint, out SynapseConnection? old))
            {
                if (old.ConnectionsIndex is not SynapseConnection.UnsetConnectionsIndex)
                {
                    int lastIndex = _connections.Count - 1;

                    if (old.ConnectionsIndex < lastIndex)
                    {
                        SynapseConnection swapped = _connections[lastIndex];
                        swapped.ConnectionsIndex = old.ConnectionsIndex;
                        _connections[old.ConnectionsIndex] = swapped;
                    }

                    _connections.RemoveAt(old.ConnectionsIndex < lastIndex ? old.ConnectionsIndex : lastIndex);
                    old.ConnectionsIndex = SynapseConnection.UnsetConnectionsIndex;
                }

                _connectionsBySignature.TryRemove(old.Signature, out _);
            }

            SynapseConnection synapseConnection = ResettableObjectPool<SynapseConnection>.Rent();
            int connectionsIndex = _connections.Count;
            synapseConnection.Initialize(endPoint, signature, connectionsIndex);

            _connectionsByEndPoint[endPoint] = synapseConnection;
            _connections.Add(synapseConnection);

            if (!_connectionsBySignature.TryAdd(signature, synapseConnection))
                _connectionsBySignature[signature] = synapseConnection;

            return synapseConnection;
        }

        /// <summary>
        /// Removes and returns a connection by endpoint.
        /// </summary>
        /// <param name="endPoint">The remote endpoint of the connection to remove.</param>
        /// <param name="removedSynapseConnection">When this method returns, contains the removed connection, or null if not found.</param>
        /// <returns>True if the connection was found and removed; otherwise false.</returns>
        public bool Remove(IPEndPoint endPoint, out SynapseConnection? removedSynapseConnection)
        {
            bool isRemoved = _connectionsByEndPoint.TryRemove(endPoint, out removedSynapseConnection);

            if (isRemoved && removedSynapseConnection is not null)
            {
                _connectionsBySignature.TryRemove(removedSynapseConnection.Signature, out _);

                int connectionsIndex = removedSynapseConnection.ConnectionsIndex;
                if (connectionsIndex is not SynapseConnection.UnsetConnectionsIndex)
                {
                    int lastConnectionsIndex = _connections.Count - 1;

                    /* If connectionsIndex is the not last entry then
                     * move the last connections entry to connectionsIndex
                     * and update the ConnectionsIndex member for the moved
                     * connection. */
                    if (connectionsIndex < lastConnectionsIndex)
                    {
                        SynapseConnection otherConnection = _connections[lastConnectionsIndex];
                        otherConnection.ConnectionsIndex = connectionsIndex;

                        _connections[connectionsIndex] = otherConnection;
                        removedSynapseConnection.ConnectionsIndex = SynapseConnection.UnsetConnectionsIndex;
                    }

                    _connections.RemoveAt(connectionsIndex);
                }
            }

            return isRemoved;
        }

        /// <summary>
        /// Key wrapper that compares IPEndPoint by address+port without boxing.
        /// </summary>
        private readonly struct EndPointKey : IEquatable<EndPointKey>
        {
            /// <summary>
            /// The wrapped remote endpoint.
            /// </summary>
            private readonly IPEndPoint _endPoint;

            /// <summary>
            /// Initializes a new <see cref="EndPointKey"/> wrapping the given endpoint.
            /// </summary>
            /// <param name="endPoint">The remote endpoint to wrap.</param>
            public EndPointKey(IPEndPoint endPoint)
            {
                _endPoint = endPoint;
            }

            /// <summary>
            /// Compares this key to another by address and port.
            /// </summary>
            /// <param name="other">The other key to compare against.</param>
            /// <returns>True if both keys represent the same address and port; otherwise false.</returns>
            public bool Equals(EndPointKey other)
            {
                if (_endPoint is null || other._endPoint is null)
                    return ReferenceEquals(_endPoint, other._endPoint);

                return _endPoint.Port == other._endPoint.Port && _endPoint.Address.Equals(other._endPoint.Address);
            }

            /// <summary>
            /// Compares this key to a boxed object.
            /// </summary>
            /// <param name="obj">The object to compare against.</param>
            /// <returns>True if <paramref name="obj"/> is an <see cref="EndPointKey"/> that equals this instance; otherwise false.</returns>
            public override bool Equals(object? obj) => obj is EndPointKey endPointKey && Equals(endPointKey);

            /// <summary>
            /// Returns a hash code derived from the endpoint's address and port.
            /// </summary>
            /// <returns>A 32-bit signed integer hash code.</returns>
            public override int GetHashCode() => _endPoint is null ? 0 : unchecked(_endPoint.Address.GetHashCode() * 397) ^ _endPoint.Port;
        }
    }
}
