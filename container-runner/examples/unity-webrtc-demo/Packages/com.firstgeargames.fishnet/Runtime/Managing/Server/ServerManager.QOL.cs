using FishNet.Connection;
using FishNet.Managing.Logging;
using FishNet.Managing.Transporting;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Transporting.Multipass;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Managing.Server
{
    public sealed partial class ServerManager : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// Called when a client is removed from the server using Kick. This is invoked before the client is disconnected.
        /// NetworkConnection when available, clientId, and KickReason are provided.
        /// </summary>
        public event Action<NetworkConnection, int, KickReason> OnClientKick;
        #endregion

        /// <summary>
        /// Stores a cache and returns a boolean result.
        /// </summary>
        /// <returns></returns>
        private bool StoreTransportCacheAndReturn(List<Transport> cache, bool returnedValue)
        {
            CollectionCaches<Transport>.Store(cache);
            return returnedValue;
        }

        /// <summary>
        /// Returns true if all server sockets have a local connection state of stopped.
        /// </summary>
        public bool AreAllServersStopped()
        {
            List<Transport> transports = NetworkManager.TransportManager.GetAllTransports(includeMultipass: false);

            foreach (Transport t in transports)
            {
                if (t.GetConnectionState(server: true) != LocalConnectionState.Stopped)
                    return StoreTransportCacheAndReturn(transports, returnedValue: false);
            }

            return StoreTransportCacheAndReturn(transports, returnedValue: true);
        }

        /// <summary>
        /// Returns true if only one server is started.
        /// </summary>
        /// <returns></returns>
        public bool IsOnlyOneServerStarted()
        {
            List<Transport> transports = NetworkManager.TransportManager.GetAllTransports(includeMultipass: false);

            int startedCount = 0;

            foreach (Transport t in transports)
            {
                if (t.GetConnectionState(true) == LocalConnectionState.Started)
                    startedCount++;
            }

            return StoreTransportCacheAndReturn(transports, startedCount == 1);
        }

        [Obsolete("Use IsOnlyOneServerStarted().")]
        public bool OneServerStarted() => IsOnlyOneServerStarted();

        /// <summary>
        /// Returns true if any server socket is in the started state.
        /// </summary>
        /// <param name = "excludedTransport">When set the transport will be ignored. This value is only used with Multipass.</param>
        public bool IsAnyServerStarted(Transport excludedTransport)
        {
            List<Transport> transports = NetworkManager.TransportManager.GetAllTransports(includeMultipass: false);

            foreach (Transport t in transports)
            {
                if (t == excludedTransport)
                    continue;
                // Another transport is started, no need to load start scenes again.
                if (t.GetConnectionState(true) == LocalConnectionState.Started)
                    return StoreTransportCacheAndReturn(transports, returnedValue: true);
            }

            return StoreTransportCacheAndReturn(transports, returnedValue: false);
        }

        /// <summary>
        /// Returns true if any server socket is in the started state.
        /// </summary>
        /// <param name = "excludedIndex">When set the transport on this index will be ignored. This value is only used with Multipass.</param>
        public bool IsAnyServerStarted(int excludedIndex = TransportConsts.UNSET_TRANSPORT_INDEX)
        {
            Transport excludedTransport = null;
            if (excludedIndex != TransportConsts.UNSET_TRANSPORT_INDEX)
            {
                if (NetworkManager.TransportManager.Transport is Multipass mp)
                    excludedTransport = mp.GetTransport(excludedIndex);
            }

            return IsAnyServerStarted(excludedTransport);
        }

        [Obsolete("Use IsAnyServerStarted.")]
        public bool AnyServerStarted(int excludedIndex = TransportConsts.UNSET_TRANSPORT_INDEX) => IsAnyServerStarted(excludedIndex);

        /// <summary>
        /// Spawns an object over the network. Can only be called on the server.
        /// </summary>
        /// <param name = "go">GameObject instance to spawn.</param>
        /// <param name = "ownerConnection">Connection to give ownership to.</param>
        public void Spawn(GameObject go, NetworkConnection ownerConnection = null, UnityEngine.SceneManagement.Scene scene = default)
        {
            if (go == null)
            {
                NetworkManager.LogWarning($"GameObject cannot be spawned because it is null.");
                return;
            }

            NetworkObject nob = go.GetComponent<NetworkObject>();
            Spawn(nob, ownerConnection, scene);
        }

        /// <summary>
        /// Spawns an object over the network. Can only be called on the server.
        /// </summary>
        /// <param name = "nob">MetworkObject instance to spawn.</param>
        /// <param name = "ownerConnection">Connection to give ownership to.</param>
        public void Spawn(NetworkObject nob, NetworkConnection ownerConnection = null, UnityEngine.SceneManagement.Scene scene = default)
        {
            if (!nob.GetIsSpawnable())
            {
                NetworkManager.LogWarning($"NetworkObject {nob} cannot be spawned because it is not marked as spawnable.");
                return;
            }
            Objects.Spawn(nob, ownerConnection, scene);
        }

        /// <summary>
        /// Despawns an object over the network. Can only be called on the server.
        /// </summary>
        /// <param name = "go">GameObject instance to despawn.</param>
        /// <param name = "cacheOnDespawnOverride">Overrides the default DisableOnDespawn value for this single despawn. Scene objects will never be destroyed.</param>
        public void Despawn(GameObject go, DespawnType? despawnType = null)
        {
            if (go == null)
            {
                NetworkManager.LogWarning($"GameObject cannot be despawned because it is null.");
                return;
            }

            NetworkObject nob = go.GetComponent<NetworkObject>();
            Despawn(nob, despawnType);
        }

        /// <summary>
        /// Despawns an object over the network. Can only be called on the server.
        /// </summary>
        /// <param name = "networkObject">NetworkObject instance to despawn.</param>
        /// <param name = "despawnType">Despawn override type.</param>
        public void Despawn(NetworkObject networkObject, DespawnType? despawnType = null)
        {
            DespawnType resolvedDespawnType = !despawnType.HasValue ? networkObject.GetDefaultDespawnType() : despawnType.Value;

            Objects.Despawn(networkObject, resolvedDespawnType, asServer: true);
        }

        /// <summary>
        /// Kicks a connection immediately while invoking OnClientKick.
        /// </summary>
        /// <param name = "conn">Client to kick.</param>
        /// <param name = "kickReason">Reason client is being kicked.</param>
        /// <param name = "loggingType">How to print logging as.</param>
        /// <param name = "log">Optional message to be debug logged.</param>
        public void Kick(NetworkConnection conn, KickReason kickReason, LoggingType loggingType = LoggingType.Common, string log = "")
        {
            if (!conn.IsValid)
                return;

            OnClientKick?.Invoke(conn, conn.ClientId, kickReason);
            if (conn.IsActive)
                conn.Disconnect(true);

            if (!string.IsNullOrEmpty(log))
                NetworkManager.Log(loggingType, log);
        }

        /// <summary>
        /// Kicks a connection immediately while invoking OnClientKick.
        /// </summary>
        /// <param name = "clientId">ClientId to kick.</param>
        /// <param name = "kickReason">Reason client is being kicked.</param>
        /// <param name = "loggingType">How to print logging as.</param>
        /// <param name = "log">Optional message to be debug logged.</param>
        public void Kick(int clientId, KickReason kickReason, LoggingType loggingType = LoggingType.Common, string log = "")
        {
            OnClientKick?.Invoke(null, clientId, kickReason);
            NetworkManager.TransportManager.Transport.StopConnection(clientId, true);
            if (!string.IsNullOrEmpty(log))
                NetworkManager.Log(loggingType, log);
        }

        /// <summary>
        /// Kicks a connection immediately while invoking OnClientKick.
        /// </summary>
        /// <param name = "conn">Client to kick.</param>
        /// <param name = "reader">Reader to clear before kicking.</param>
        /// <param name = "kickReason">Reason client is being kicked.</param>
        /// <param name = "loggingType">How to print logging as.</param>
        /// <param name = "log">Optional message to be debug logged.</param>
        public void Kick(NetworkConnection conn, Reader reader, KickReason kickReason, LoggingType loggingType = LoggingType.Common, string log = "")
        {
            reader.Clear();
            Kick(conn, kickReason, loggingType, log);
        }
    }
}