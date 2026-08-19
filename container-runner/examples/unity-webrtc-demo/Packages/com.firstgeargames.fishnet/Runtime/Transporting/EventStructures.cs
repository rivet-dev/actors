using System;
using System.Collections.Generic;

namespace FishNet.Transporting
{
    /// <summary>
    /// Container for connected clients state for a client.
    /// </summary>
    public struct ConnectedClientsArgs
    {
        /// <summary>
        /// Collection of client ids connected to the server.
        /// </summary>
        public List<int> ClientIds { get; private set; }

        public ConnectedClientsArgs(List<int> clientIds)
        {
            ClientIds = clientIds;
        }
    }

    /// <summary>
    /// Container about data received on the server.
    /// </summary>
    public struct ServerReceivedDataArgs
    {
        /// <summary>
        /// Data received.
        /// </summary>
        public ArraySegment<byte> Data;
        /// <summary>
        /// Channel data was received on.
        /// </summary>
        public Channel Channel;
        /// <summary>
        /// ConnectionId from which client sent data, if data was received on the server.
        /// </summary>
        public int ConnectionId;
        /// <summary>
        /// Index of the transport that is for.
        /// This is primarily used when supporting multiple transports.
        /// </summary>
        public int TransportIndex;
        /// <summary>
        /// Delegate to invoke after data is processed.
        /// </summary>
        /// <returns></returns>
        public Action FinalizeMethod;

        public ServerReceivedDataArgs(ArraySegment<byte> data, Channel channel, int connectionId, int transportIndex)
        {
            Data = data;
            Channel = channel;
            ConnectionId = connectionId;
            TransportIndex = transportIndex;
            FinalizeMethod = null;
        }

        public ServerReceivedDataArgs(ArraySegment<byte> data, Channel channel, int connectionId, int transportIndex, Action finalizeMethod)
        {
            Data = data;
            Channel = channel;
            ConnectionId = connectionId;
            TransportIndex = transportIndex;
            FinalizeMethod = finalizeMethod;
        }
    }

    /// <summary>
    /// Container about data received on the local client.
    /// </summary>
    public struct ClientReceivedDataArgs
    {
        /// <summary>
        /// Data received.
        /// </summary>
        public ArraySegment<byte> Data;
        /// <summary>
        /// Channel data was received on.
        /// </summary>
        public Channel Channel;
        /// <summary>
        /// Index of the transport that is for.
        /// This is primarily used when supporting multiple transports.
        /// </summary>
        public int TransportIndex;

        public ClientReceivedDataArgs(ArraySegment<byte> data, Channel channel, int transportIndex)
        {
            Data = data;
            Channel = channel;
            TransportIndex = transportIndex;
        }
    }

    /// <summary>
    /// Container about a connection state change for a client.
    /// </summary>
    public struct RemoteConnectionStateArgs
    {
        /// <summary>
        /// Index of the transport that is for.
        /// This is primarily used when supporting multiple transports.
        /// </summary>
        public int TransportIndex;
        /// <summary>
        /// New connection state.
        /// </summary>
        public RemoteConnectionState ConnectionState;
        /// <summary>
        /// ConnectionId for which client the state changed. Will be -1 if ConnectionState was for the local server.
        /// </summary>
        public int ConnectionId;

        public RemoteConnectionStateArgs(RemoteConnectionState connectionState, int connectionId, int transportIndex)
        {
            ConnectionState = connectionState;
            ConnectionId = connectionId;
            TransportIndex = transportIndex;
        }
    }

    /// <summary>
    /// Container about a connection state change for the server.
    /// </summary>
    public struct ServerConnectionStateArgs
    {
        /// <summary>
        /// Index of the transport that is for.
        /// This is primarily used when supporting multiple transports.
        /// </summary>
        public int TransportIndex;
        /// <summary>
        /// New connection state.
        /// </summary>
        public LocalConnectionState ConnectionState;

        public ServerConnectionStateArgs(LocalConnectionState connectionState, int transportIndex)
        {
            ConnectionState = connectionState;
            TransportIndex = transportIndex;
        }
    }

    /// <summary>
    /// Container about a connection state change for the local client.
    /// </summary>
    public struct ClientConnectionStateArgs
    {
        /// <summary>
        /// New connection state.
        /// </summary>
        public LocalConnectionState ConnectionState;
        /// <summary>
        /// Index of the transport that is for.
        /// This is primarily used when supporting multiple transports.
        /// </summary>
        public int TransportIndex;

        public ClientConnectionStateArgs(LocalConnectionState connectionState, int transportIndex)
        {
            ConnectionState = connectionState;
            TransportIndex = transportIndex;
        }
    }
}