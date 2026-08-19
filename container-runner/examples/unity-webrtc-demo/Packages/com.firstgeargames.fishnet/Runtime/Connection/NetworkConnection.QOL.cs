using FishNet.Managing;
using FishNet.Managing.Logging;
using FishNet.Managing.Server;
using FishNet.Serializing;
using System;
using UnityEngine;

namespace FishNet.Connection
{
    /// <summary>
    /// A container for a connected client used to perform actions on and gather information for the declared client.
    /// </summary>
    public partial class NetworkConnection
    {
        #region Public.
        /// <summary>
        /// Returns true if this connection is a clientHost.
        /// </summary>
        public bool IsHost => NetworkManager == null ? false : NetworkManager.IsServerStarted && this == NetworkManager.ClientManager.Connection;
        /// <summary>
        /// Returns if this connection is for the local client.
        /// </summary>
        public bool IsLocalClient => NetworkManager == null ? false : NetworkManager.ClientManager.Connection == this;
        #endregion

        /// <summary>
        /// Returns the address of this connection.
        /// </summary>
        /// <returns></returns>
        public string GetAddress()
        {
            if (!IsValid)
                return string.Empty;
            if (NetworkManager == null)
                return string.Empty;

            return NetworkManager.TransportManager.Transport.GetConnectionAddress(ClientId);
        }

        /// <summary>
        /// Kicks a connection immediately while invoking OnClientKick.
        /// </summary>
        /// <param name = "kickReason">Reason client is being kicked.</param>
        /// <param name = "loggingType">How to print logging as.</param>
        /// <param name = "log">Optional message to be debug logged.</param>
        public void Kick(KickReason kickReason, LoggingType loggingType = LoggingType.Common, string log = "")
        {
            if (CanKick())
                NetworkManager.ServerManager.Kick(this, kickReason, loggingType, log);
        }

        /// <summary>
        /// Kicks a connection immediately while invoking OnClientKick.
        /// </summary>
        /// <param name = "reader">Reader to clear before kicking.</param>
        /// <param name = "kickReason">Reason client is being kicked.</param>
        /// <param name = "loggingType">How to print logging as.</param>
        /// <param name = "log">Optional message to be debug logged.</param>
        public void Kick(Reader reader, KickReason kickReason, LoggingType loggingType = LoggingType.Common, string log = "")
        {
            if (CanKick())
                NetworkManager.ServerManager.Kick(this, reader, kickReason, loggingType, log);
        }

        private bool CanKick()
        {
            // Connection isn't valid, calling kick on an empty connection.
            if (!IsValid)
                return false;

            // Should never happen.
            if (NetworkManager == null)
            {
                NetworkManager = InstanceFinder.NetworkManager;
                NetworkManager.LogError($"NetworkManager was not set for connection {ToString()}. InstanceFinder has been used.");
            }

            return true;
        }
    }
}