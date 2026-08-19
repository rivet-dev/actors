#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Broadcast;
using FishNet.Broadcast.Helping;
using FishNet.Connection;
using FishNet.Managing.Logging;
using FishNet.Managing.Utility;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Serializing.Helping;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using FishNet.Managing.Transporting;
using UnityEngine;

namespace FishNet.Managing.Server
{
    public sealed partial class ServerManager : MonoBehaviour
    {
        #region Private.
        /// <summary>
        /// Handler for registered broadcasts.
        /// </summary>
        private readonly Dictionary<ushort, BroadcastHandlerBase> _broadcastHandlers = new();
        /// <summary>
        /// Connections which can be broadcasted to after having excluded removed.
        /// </summary>
        private HashSet<NetworkConnection> _connectionsWithoutExclusionsCache = new();
        #endregion

        /// <summary>
        /// Registers a method to call when a Broadcast arrives.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast being registered.</typeparam>
        /// <param name = "handler">Method to call.</param>
        /// <param name = "requireAuthentication">True if the client must be authenticated for the method to call.</param>
        public void RegisterBroadcast<T>(Action<NetworkConnection, T, Channel> handler, bool requireAuthentication = true) where T : struct, IBroadcast
        {
            if (handler == null)
            {
                NetworkManager.LogError($"Broadcast cannot be registered because handler is null. This may occur when trying to register to objects which require initialization, such as events.");
                return;
            }

            ushort key = BroadcastExtensions.GetKey<T>();

#if DEVELOPMENT && !UNITY_SERVER
            NetworkManager.SetBroadcastName<T>(key);
#endif

            // Create new IBroadcastHandler if needed.
            BroadcastHandlerBase bhs;
            if (!_broadcastHandlers.TryGetValueIL2CPP(key, out bhs))
            {
                bhs = new ClientBroadcastHandler<T>(requireAuthentication);
                _broadcastHandlers.Add(key, bhs);
            }
            // Register handler to IBroadcastHandler.
            bhs.RegisterHandler(handler);
        }

        /// <summary>
        /// Unregisters a method call from a Broadcast type.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast being unregistered.</typeparam>
        /// <param name = "handler">Method to unregister.</param>
        public void UnregisterBroadcast<T>(Action<NetworkConnection, T, Channel> handler) where T : struct, IBroadcast
        {
            ushort key = BroadcastExtensions.GetKey<T>();
            if (_broadcastHandlers.TryGetValueIL2CPP(key, out BroadcastHandlerBase bhs))
                bhs.UnregisterHandler(handler);
        }

        /// <summary>
        /// Parses a received broadcast.
        /// </summary>
        private void ParseBroadcast(PooledReader reader, NetworkConnection conn, Channel channel)
        {
            int readerPositionAfterDebug = reader.Position;

            ushort key = reader.ReadUInt16();
            int dataLength = Packets.GetPacketLength((ushort)PacketId.Broadcast, reader, channel);

            // try to invoke the handler for that message
            if (_broadcastHandlers.TryGetValueIL2CPP(key, out BroadcastHandlerBase bhs))
            {
                if (bhs.RequireAuthentication && !conn.IsAuthenticated)
                    conn.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"ConnectionId {conn.ClientId} sent a broadcast which requires authentication, but client was not authenticated. Client has been disconnected.");
                else
                    bhs.InvokeHandlers(conn, reader, channel);
            }
            else
            {
                reader.Skip(dataLength);
            }

#if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
            {
                string broadcastName = NetworkManager.GetBroadcastName(key);
                _networkTrafficStatistics.AddInboundPacketIdData(PacketId.Broadcast, broadcastName, reader.Position - readerPositionAfterDebug + TransportManager.PACKETID_LENGTH,gameObject: null,  asServer: true);
            }
#endif
        }

        /// <summary>
        /// Sends a broadcast to a connection.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "connection">Connection to send to.</param>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the client must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void Broadcast<T>(NetworkConnection connection, T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!Started)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because server is not active.");
                return;
            }
            if (requireAuthenticated && !connection.IsAuthenticated)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because they are not authenticated.");
                return;
            }

            PooledWriter writer = WriterPool.Retrieve();
            BroadcastsSerializers.WriteBroadcast(writer, message);
            ArraySegment<byte> segment = writer.GetArraySegment();

            AddOutboundNetworkTraffic<T>(segment.Count);

            NetworkManager.TransportManager.SendToClient((byte)channel, segment, connection);
            writer.Store();
        }

        /// <summary>
        /// Sends a broadcast to connections.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "connections">Connections to send to.</param>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the clients must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void Broadcast<T>(HashSet<NetworkConnection> connections, T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!Started)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because server is not active.");
                return;
            }

            bool failedAuthentication = false;
            PooledWriter writer = WriterPool.Retrieve();
            BroadcastsSerializers.WriteBroadcast(writer, message);
            ArraySegment<byte> segment = writer.GetArraySegment();

            int sentBytes = 0;
            int segmentCount = segment.Count;

            foreach (NetworkConnection conn in connections)
            {
                if (requireAuthenticated && !conn.IsAuthenticated)
                {
                    failedAuthentication = true;
                }
                else
                {
                    NetworkManager.TransportManager.SendToClient((byte)channel, segment, conn);
                    sentBytes += segmentCount;
                }
            }

            writer.Store();

            AddOutboundNetworkTraffic<T>(sentBytes);

            if (failedAuthentication)
                NetworkManager.LogWarning($"One or more broadcast did not send to a client because they were not authenticated.");
        }

        /// <summary>
        /// Sends a broadcast to connections except excluded.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "connections">Connections to send to.</param>
        /// <param name = "excludedConnection">Connection to exclude.</param>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the clients must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void BroadcastExcept<T>(HashSet<NetworkConnection> connections, NetworkConnection excludedConnection, T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!Started)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because server is not active.");
                return;
            }

            // Fast exit if no exclusions.
            if (excludedConnection == null || !excludedConnection.IsValid)
            {
                Broadcast(connections, message, requireAuthenticated, channel);
                return;
            }

            connections.Remove(excludedConnection);
            Broadcast(connections, message, requireAuthenticated, channel);
        }

        /// <summary>
        /// Sends a broadcast to connections except excluded.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "connections">Connections to send to.</param>
        /// <param name = "excludedConnections">Connections to exclude.</param>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the clients must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void BroadcastExcept<T>(HashSet<NetworkConnection> connections, HashSet<NetworkConnection> excludedConnections, T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!Started)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because server is not active.");
                return;
            }

            // Fast exit if no exclusions.
            if (excludedConnections == null || excludedConnections.Count == 0)
            {
                Broadcast(connections, message, requireAuthenticated, channel);
                return;
            }

            /* I'm not sure if the hashset API such as intersect generates
             * GC or not but I'm betting doing remove locally is faster, or
             * just as fast. */
            foreach (NetworkConnection ec in excludedConnections)
                connections.Remove(ec);

            Broadcast(connections, message, requireAuthenticated, channel);
        }

        /// <summary>
        /// Sends a broadcast to all connections except excluded.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "excludedConnection">Connection to exclude.</param>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the clients must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void BroadcastExcept<T>(NetworkConnection excludedConnection, T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!Started)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because server is not active.");
                return;
            }

            // Fast exit if there are no excluded.
            if (excludedConnection == null || !excludedConnection.IsValid)
            {
                Broadcast(message, requireAuthenticated, channel);
                return;
            }

            _connectionsWithoutExclusionsCache.Clear();
            /* It will be faster to fill the entire list then
             * remove vs checking if each connection is contained within excluded. */
            foreach (NetworkConnection c in Clients.Values)
                _connectionsWithoutExclusionsCache.Add(c);
            //Remove
            _connectionsWithoutExclusionsCache.Remove(excludedConnection);

            Broadcast(_connectionsWithoutExclusionsCache, message, requireAuthenticated, channel);
        }

        /// <summary>
        /// Sends a broadcast to all connections except excluded.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "excludedConnections">Connections to send to.</param>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the clients must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void BroadcastExcept<T>(HashSet<NetworkConnection> excludedConnections, T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!Started)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because server is not active.");
                return;
            }

            //Fast exit if there are no excluded.
            if (excludedConnections == null || excludedConnections.Count == 0)
            {
                Broadcast(message, requireAuthenticated, channel);
                return;
            }

            _connectionsWithoutExclusionsCache.Clear();
            /* It will be faster to fill the entire list then
             * remove vs checking if each connection is contained within excluded. */
            foreach (NetworkConnection c in Clients.Values)
                _connectionsWithoutExclusionsCache.Add(c);
            //Remove
            foreach (NetworkConnection c in excludedConnections)
                _connectionsWithoutExclusionsCache.Remove(c);

            Broadcast(_connectionsWithoutExclusionsCache, message, requireAuthenticated, channel);
        }

        /// <summary>
        /// Sends a broadcast to observers.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "networkObject">NetworkObject to use Observers from.</param>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the clients must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void Broadcast<T>(NetworkObject networkObject, T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (networkObject == null)
            {
                NetworkManager.LogWarning($"Cannot send broadcast because networkObject is null.");
                return;
            }

            Broadcast(networkObject.Observers, message, requireAuthenticated, channel);
        }

        /// <summary>
        /// Sends a broadcast to all clients.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the clients must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void Broadcast<T>(T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!Started)
            {
                NetworkManager.LogWarning($"Cannot send broadcast to client because server is not active.");
                return;
            }

            bool failedAuthentication = false;
            PooledWriter writer = WriterPool.Retrieve();
            BroadcastsSerializers.WriteBroadcast(writer, message);
            ArraySegment<byte> segment = writer.GetArraySegment();

            int sentBytes = 0;
            int segmentCount = segment.Count;

            foreach (NetworkConnection conn in Clients.Values)
            {
                //
                if (requireAuthenticated && !conn.IsAuthenticated)
                {
                    failedAuthentication = true;
                }
                else
                {
                    NetworkManager.TransportManager.SendToClient((byte)channel, segment, conn);
                    sentBytes += segmentCount;
                }
            }

            AddOutboundNetworkTraffic<T>(sentBytes);

            writer.Store();

            if (failedAuthentication)
                NetworkManager.LogWarning($"One or more broadcast did not send to a client because they were not authenticated.");
        }

        /// <summary>
        /// Adds data for an outbound broadcast.
        /// </summary>
        private void AddOutboundNetworkTraffic<T>(int bytes) where T : struct, IBroadcast
        {
#if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
            {
                if (bytes <= 0)
                    return;

                ushort key = BroadcastExtensions.GetKey<T>();
                string broadcastName = NetworkManager.GetBroadcastName(key);

                /* Do not include packetId length -- its written in the 'WriteBroadcast' method. */
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.Broadcast, broadcastName, bytes, gameObject: null, asServer: true);
            }
#endif
        }
    }
}