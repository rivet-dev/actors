using FishNet.Connection;
using FishNet.Documenting;
using FishNet.Object.Synchronizing.Internal;
using FishNet.Serializing;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Object
{
    public abstract partial class NetworkBehaviour : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// True if OnStartServer has been called.
        /// </summary>
        [APIExclude]
        public bool OnStartServerCalled { get; private set; }
        /// <summary>
        /// True if OnStartClient has been called.
        /// </summary>
        [APIExclude]
        public bool OnStartClientCalled { get; private set; }
        #endregion

        #region Private.
        /// <summary>
        /// True if OnStartNetwork has been called.
        /// </summary>
        private bool _onStartNetworkCalled;
        /// <summary>
        /// True if OnStopNetwork has been called.
        /// </summary>
        private bool _onStopNetworkCalled;
        #endregion

        /* Payloads are written and read immediatley after the header containing the target NetworkObject/Behaviour. */
        /// <summary>
        /// Called when writing a spawn. This may be used to deliver information for predicted spawning, or simply have values set before initialization without depending on SyncTypes.
        /// </summary>
        /// <param name = "connection">Connection receiving the payload. When sending to the server connection.IsValid will return false.</param>
        public virtual void WritePayload(NetworkConnection connection, Writer writer) { }

        /// <summary>
        /// Called before network start callbacks, but after the object is initialized with network values. This may be used to read information from predicted spawning, or simply have values set before initialization without depending on SyncTypes.
        /// </summary>
        /// <param name = "connection">Connection sending the payload. When coming from the server connection.IsValid will return false.</param>
        public virtual void ReadPayload(NetworkConnection connection, Reader reader) { }

        /// <summary>
        /// Invokes OnStartXXXX for synctypes, letting them know the NetworkBehaviour start cycle has been completed.
        /// </summary>
        internal void InvokeSyncTypeOnStartCallbacks(bool asServer)
        {
            foreach (SyncBase item in _syncTypes.Values)
                item.OnStartCallback(asServer);
        }

        /// <summary>
        /// Invokes OnStopXXXX for synctypes, letting them know the NetworkBehaviour stop cycle is about to start.
        /// </summary>
        internal void InvokeSyncTypeOnStopCallbacks(bool asServer)
        {
            // if (_syncTypes == null)
            //     return;
            foreach (SyncBase item in _syncTypes.Values)
                item.OnStopCallback(asServer);
        }

        /// <summary>
        /// Invokes the OnStart/StopNetwork.
        /// </summary>
        internal void InvokeOnNetwork_Internal(bool start)
        {
            if (start)
            {
                if (_onStartNetworkCalled)
                    return;

                if (!gameObject.activeInHierarchy)
                {
                    NetworkInitialize___Early();
                    NetworkInitialize___Late();
                }
                OnStartNetwork_Internal();
            }
            else
            {
                if (_onStopNetworkCalled)
                    return;
                OnStopNetwork_Internal();
            }
        }

        internal virtual void OnStartNetwork_Internal()
        {
            _onStartNetworkCalled = true;
            _onStopNetworkCalled = false;
            OnStartNetwork();
        }

        /// <summary>
        /// Called when the network has initialized this object. May be called for server or client but will only be called once.
        /// When as host or server this method will run before OnStartServer.
        /// When as client only the method will run before OnStartClient.
        /// </summary>
        public virtual void OnStartNetwork() { }

        internal virtual void OnStopNetwork_Internal()
        {
            _onStopNetworkCalled = true;
            _onStartNetworkCalled = false;

            OnStopNetwork();
        }

        /// <summary>
        /// Called when the network is deinitializing this object. May be called for server or client but will only be called once.
        /// When as host or server this method will run after OnStopServer.
        /// When as client only this method will run after OnStopClient.
        /// </summary>
        public virtual void OnStopNetwork() { }

        internal void OnStartServer_Internal()
        {
            OnStartServerCalled = true;
            OnStartServer();
        }

        /// <summary>
        /// Called on the server after initializing this object.
        /// SyncTypes modified before or during this method will be sent to clients in the spawn message.
        /// </summary>
        public virtual void OnStartServer() { }

        internal void OnStopServer_Internal()
        {
            OnStartServerCalled = false;
            ReturnRpcLinks();
            OnStopServer();
        }

        /// <summary>
        /// Called on the server before deinitializing this object.
        /// </summary>
        public virtual void OnStopServer() { }

        internal void OnOwnershipServer_Internal(NetworkConnection prevOwner)
        {
            ResetState_Prediction(true);
            OnOwnershipServer(prevOwner);
        }

        /// <summary>
        /// Called on the server after ownership has changed.
        /// </summary>
        /// <param name = "prevOwner">Previous owner of this object.</param>
        public virtual void OnOwnershipServer(NetworkConnection prevOwner) { }

        /// <summary>
        /// Called on the server after a spawn message for this object has been sent to clients.
        /// Useful for sending remote calls or data to clients.
        /// </summary>
        /// <param name = "connection">Connection the object is being spawned for.</param>
        public virtual void OnSpawnServer(NetworkConnection connection) { }

        /// <summary>
        /// Called on the server before a despawn message for this object has been sent to connection.
        /// Useful for sending remote calls or actions to clients.
        /// </summary>
        public virtual void OnDespawnServer(NetworkConnection connection) { }

        internal void OnStartClient_Internal()
        {
            OnStartClientCalled = true;
            OnStartClient();
        }

        /// <summary>
        /// Called on the client after initializing this object.
        /// </summary>
        public virtual void OnStartClient() { }

        internal void OnStopClient_Internal()
        {
            OnStartClientCalled = false;
            OnStopClient();
        }

        /// <summary>
        /// Called on the client before deinitializing this object.
        /// </summary>
        public virtual void OnStopClient() { }

        internal void OnOwnershipClient_Internal(NetworkConnection prevOwner)
        {
            // If losing or gaining ownership then clear replicate cache.
            if (IsOwner || prevOwner == LocalConnection)
            {
                ResetState_Prediction(false);
            }

            OnOwnershipClient(prevOwner);
        }

        /// <summary>
        /// Called on the client after gaining or losing ownership.
        /// </summary>
        /// <param name = "prevOwner">Previous owner of this object.</param>
        public virtual void OnOwnershipClient(NetworkConnection prevOwner) { }
    }
}