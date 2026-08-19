using FishNet.Managing;
using FishNet.Object;
using System.Collections.Generic;
using FishNet.Managing.Server;
using FishNet.Transporting;
using UnityEngine;

namespace FishNet.Component.Spawning
{
    /// <summary>
    /// Spawns network objects when the server starts.
    /// </summary>
    [AddComponentMenu("FishNet/Component/ServerSpawner")]
    public class ServerSpawner : MonoBehaviour
    {
        #region Serialized
        [Tooltip("True to spawn the objects as soon as the server starts. False if you wish to call Spawn manually.")]
        [SerializeField]
        private bool _automaticallySpawn = true;
        /// <summary>
        /// NetworkObjects to spawn when the server starts.
        /// </summary>
        [Tooltip("NetworkObjects to spawn when the server starts.")]
        [SerializeField]
        private List<NetworkObject> _networkObjects = new();
        #endregion

        #region Private.
        /// <summary>
        /// First instance of the ServerManager found. This will be either the ServerManager on or above this object, or InstanceFinder.ServerManager.
        /// </summary>
        private ServerManager _serverManager;
        #endregion

        private void Awake()
        {
            InitializeOnce();
        }

        private void OnDestroy()
        {
            if (_serverManager == null)
                return;

            // Unsubscribe even if not automatically spawning; this is to protect against the user unchecking during play mode.
            _serverManager.OnServerConnectionState -= ServerManager_OnServerConnectionState;
        }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        private void InitializeOnce()
        {
            _serverManager = GetComponentInParent<ServerManager>();
            if (_serverManager == null)
                _serverManager = InstanceFinder.ServerManager;

            if (_serverManager == null)
            {
                NetworkManagerExtensions.LogWarning($"{nameof(ServerSpawner)} on {gameObject.name} cannot work as NetworkManager wasn't found on this object or within parent objects.");
                return;
            }

            // Only subscribe if to automatically spawn.
            if (_automaticallySpawn)
                _serverManager.OnServerConnectionState += ServerManager_OnServerConnectionState;
        }

        private void ServerManager_OnServerConnectionState(ServerConnectionStateArgs args)
        {
            // If not started then exit.
            if (args.ConnectionState != LocalConnectionState.Started)
                return;

            // If more than 1 server is started then exit. This means the user is using multipass and another server already started.
            if (!_serverManager.IsOnlyOneServerStarted())
                return;

            Spawn_Internally();
        }

        private void Spawn_Internally()
        {
            if (_serverManager == null)
                return;

            // Spawn the objects now.
            foreach (NetworkObject networkObject in _networkObjects)
            {
                NetworkObject nob = _serverManager.NetworkManager.GetPooledInstantiated(networkObject, asServer: true);
                _serverManager.Spawn(nob);
            }
        }

        /// <summary>
        /// Spawns all provided NetworkObjects.
        /// </summary>
        /// <remarks>This will spawn the objects again even if they were already spawned automatically or manually before.</remarks>
        public void Spawn() => Spawn_Internally();
    }
}