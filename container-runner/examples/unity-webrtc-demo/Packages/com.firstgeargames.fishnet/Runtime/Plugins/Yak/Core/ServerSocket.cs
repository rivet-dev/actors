using FishNet.Connection;
using FishNet.Transporting.Yak.Client;
using System;
using System.Collections.Generic;

namespace FishNet.Transporting.Yak.Server
{
    /// <summary>
    /// Creates a fake socket acting as server.
    /// </summary>
    public class ServerSocket : CommonSocket
    {
        #region Public.
        /// <summary>
        /// Gets the current ConnectionState of a remote client on the server.
        /// </summary>
        /// <param name = "connectionId">ConnectionId to get ConnectionState for.</param>
        internal RemoteConnectionState GetConnectionState(int connectionId)
        {
            if (connectionId != NetworkConnection.SIMULATED_CLIENTID_VALUE)
                return RemoteConnectionState.Stopped;

            LocalConnectionState state = _client.GetLocalConnectionState();
            return state == LocalConnectionState.Started ? RemoteConnectionState.Started : RemoteConnectionState.Stopped;
        }
        #endregion

        #region Private.
        /// <summary>
        /// Packets received from local client.
        /// </summary>
        private Queue<LocalPacket> _incoming = new();
        /// <summary>
        /// Socket for client.
        /// </summary>
        private ClientSocket _client;
        #endregion


        /// <summary>
        /// Starts the server.
        /// </summary>
        internal bool StartConnection()
        {
            return true;
        }


        /// <summary>
        /// Stops the local socket.
        /// </summary>
        internal bool StopConnection()
        {
            return true;
        }

        /// <summary>
        /// Stops a remote client from the server, disconnecting the client.
        /// </summary>
        /// <param name = "connectionId">ConnectionId of the client to disconnect.</param>
        internal bool StopConnection(int connectionId)
        {
            return true;
        }



        #region Local client.

        #endregion
    }
}