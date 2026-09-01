using FishNet.CodeAnalysis.Annotations;
using FishNet.Component.ColliderRollback;
using FishNet.Connection;
using FishNet.Managing;
using FishNet.Managing.Client;
using FishNet.Managing.Observing;
using FishNet.Managing.Predicting;
using FishNet.Managing.Scened;
using FishNet.Managing.Server;
using FishNet.Managing.Timing;
using FishNet.Managing.Transporting;
using FishNet.Observing;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine; 

namespace FishNet.Object
{
    public abstract partial class NetworkBehaviour : MonoBehaviour
    {
        #region Public.
        #region Obsoletes
        // Remove on v5
        [Obsolete("Use IsClientOnlyInitialized. Note the difference between IsClientOnlyInitialized and IsClientOnlyStarted.")]
        public bool IsClientOnly => IsClientOnlyInitialized;
        [Obsolete("Use IsServerOnlyInitialized. Note the difference between IsServerOnlyInitialized and IsServerOnlyStarted.")]
        public bool IsServerOnly => IsServerOnlyInitialized;
        [Obsolete("Use IsHostInitialized. Note the difference between IsHostInitialized and IsHostStarted.")]
        public bool IsHost => IsHostInitialized;
        [Obsolete("Use IsClientInitialized. Note the difference between IsClientInitialized and IsClientStarted.")]
        public bool IsClient => IsClientInitialized;
        [Obsolete("Use IsServerInitialized. Note the difference between IsServerInitialized and IsServerStarted.")]
        public bool IsServer => IsServerInitialized;
        #endregion

        /// <summary>
        /// Gets this instance as the containing NetworkObject.
        /// </summary>
        public static implicit operator NetworkObject(NetworkBehaviour nb) => nb._networkObjectCache; 
        
        /// <summary>
        /// True if the NetworkObject for this NetworkBehaviour is deinitializing.
        /// </summary>
        public bool IsDeinitializing => _networkObjectCache.IsDeinitializing;
        /// <summary>
        /// NetworkManager for this object.
        /// </summary>
        public NetworkManager NetworkManager => _networkObjectCache.NetworkManager;
        /// <summary>
        /// ServerManager for this object.
        /// </summary>
        public ServerManager ServerManager => _networkObjectCache.ServerManager;
        /// <summary>
        /// ClientManager for this object.
        /// </summary>
        public ClientManager ClientManager => _networkObjectCache.ClientManager;
        /// <summary>
        /// ObserverManager for this object.
        /// </summary>
        public ObserverManager ObserverManager => _networkObjectCache.ObserverManager;
        /// <summary>
        /// TransportManager for this object.
        /// </summary>
        public TransportManager TransportManager => _networkObjectCache.TransportManager;
        /// <summary>
        /// TimeManager for this object.
        /// </summary>
        public TimeManager TimeManager => _networkObjectCache.TimeManager;
        /// <summary>
        /// SceneManager for this object.
        /// </summary>
        public SceneManager SceneManager => _networkObjectCache.SceneManager;
        /// <summary>
        /// PredictionManager for this object.
        /// </summary>
        public PredictionManager PredictionManager => _networkObjectCache.PredictionManager;
        /// <summary>
        /// RollbackManager for this object.
        /// </summary>
        public RollbackManager RollbackManager => _networkObjectCache.RollbackManager;
        /// <summary>
        /// NetworkObserver on this object.
        /// </summary>
        public NetworkObserver NetworkObserver => _networkObjectCache.NetworkObserver;
        /// <summary>
        /// True if this object has been initialized on the client side.
        /// This is set true right before client start callbacks and after stop callbacks.
        /// </summary>
        public bool IsClientInitialized => _networkObjectCache.IsClientInitialized;
        /// <summary>
        /// True if the client is started and authenticated.
        /// </summary>
        public bool IsClientStarted => _networkObjectCache.IsClientStarted;
        /// <summary>
        /// True if this object has been initialized only on the client side.
        /// This is set true right before server start callbacks and after stop callbacks.
        public bool IsClientOnlyInitialized => _networkObjectCache.IsClientOnlyInitialized;
        /// <summary>
        /// True if only the client is started and authenticated.
        /// </summary>
        public bool IsClientOnlyStarted => _networkObjectCache.IsClientOnlyStarted;
        /// <summary>
        /// True if this object has been initialized on the server side.
        /// This is set true right before server start callbacks and after stop callbacks.
        /// </summary>
        public bool IsServerInitialized => _networkObjectCache.IsServerInitialized;
        /// <summary>
        /// True if server is started.
        /// </summary>
        public bool IsServerStarted => _networkObjectCache.IsServerStarted;
        /// <summary>
        /// True if this object has been initialized only on the server side.
        /// This is set true right before server start callbacks and after stop callbacks.
        public bool IsServerOnlyInitialized => _networkObjectCache.IsServerOnlyInitialized;
        /// <summary>
        /// True if only the server is started.
        /// </summary>
        public bool IsServerOnlyStarted => _networkObjectCache.IsServerOnlyStarted;
        /// <summary>
        /// True if this object has been initialized on the server and client side.
        /// </summary>
        public bool IsHostInitialized => _networkObjectCache.IsHostInitialized;
        /// <summary>
        /// True if client and server are started.
        /// </summary>
        public bool IsHostStarted => _networkObjectCache.IsHostStarted;
        /// <summary>
        /// True if client nor server are started.
        /// </summary>
        public bool IsOffline => _networkObjectCache.IsOffline;
        /// <summary>
        /// True if the object will always initialize as a networked object. When false the object will not automatically initialize over the network. Using Spawn() on an object will always set that instance as networked.
        /// To check if server or client has been initialized on this object use IsXYZInitialized.
        /// </summary>
        [Obsolete("Use GetIsNetworked.")] // Remove on V5.
        public bool IsNetworked => GetIsNetworked();

        /// <summary>
        /// True if the object will always initialize as a networked object. When false the object will not automatically initialize over the network. Using Spawn() on an object will always set that instance as networked.
        /// To check if server or client has been initialized on this object use IsXYZInitialized.
        /// </summary>
        public bool GetIsNetworked() => _networkObjectCache.GetIsNetworked();

        /// <summary>
        /// Sets IsNetworked value. This method must be called before Start.
        /// </summary>
        /// <param name = "value">New IsNetworked value.</param>
        public void SetIsNetworked(bool value) => _networkObjectCache.SetIsNetworked(value);

        /// <summary>
        /// True if a reconcile is occuring on the PredictionManager. Note the difference between this and IsBehaviourReconciling.
        /// </summary>
        public bool IsManagerReconciling => _networkObjectCache.IsManagerReconciling;
        /// <summary>
        /// Observers for this NetworkBehaviour.
        /// </summary>
        public HashSet<NetworkConnection> Observers => _networkObjectCache.Observers;
        /// <summary>
        /// True if the local client is the owner of this object.
        /// </summary>
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartServer", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartNetwork", " Use base.Owner.IsLocalClient instead.")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Awake", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Start", "")]
        public bool IsOwner => _networkObjectCache.IsOwner;
        /// <summary>
        /// True if IsOwner, or if IsServerInitialized with no Owner.
        /// </summary>
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartServer", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartNetwork", " Use (base.Owner.IsLocalClient || (base.IsServerInitialized && !Owner.Isvalid) instead.")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Awake", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Start", "")]
        public bool IsController => _networkObjectCache.IsOwner || (_networkObjectCache.IsServerInitialized && !_networkObjectCache.Owner.IsValid);
        [Obsolete("Use IsController.")]
        public bool HasAuthority => IsController;
        /// <summary>
        /// Owner of this object.
        /// </summary>
        public NetworkConnection Owner
        {
            get
            {
                // Ensures a null Owner is never returned.
                if (_networkObjectCache == null)
                    return NetworkManager.EmptyConnection;

                return _networkObjectCache.Owner;
            }
        }
        /// <summary>
        /// ClientId for this NetworkObject owner.
        /// </summary>
        public int OwnerId => _networkObjectCache.OwnerId;
        /// <summary>
        /// Unique Id for this _networkObjectCache. This does not represent the object owner.
        /// </summary>
        public int ObjectId => _networkObjectCache.ObjectId;
        /// <summary>
        /// The local connection of the client calling this method.
        /// </summary>
        public NetworkConnection LocalConnection => _networkObjectCache.LocalConnection;
        #endregion

        /// <summary>
        /// Returns if a connection is the owner of this object.
        /// </summary>
        /// <param name = "connection"></param>
        /// <returns></returns>
        public bool OwnerMatches(NetworkConnection connection)
        {
            return _networkObjectCache.Owner == connection;
        }

        /// <summary>
        /// Despawns a GameObject. Only call from the server.
        /// </summary>
        /// <param name = "go">GameObject to despawn.</param>
        /// <param name = "despawnType">What happens to the object after being despawned.</param>
        public void Despawn(GameObject go, DespawnType? despawnType = null)
        {
            if (!IsNetworkObjectNull(true))
                _networkObjectCache.Despawn(go, despawnType);
        }

        /// <summary>
        /// Despawns  a NetworkObject. Only call from the server.
        /// </summary>
        /// <param name = "nob">NetworkObject to despawn.</param>
        /// <param name = "despawnType">What happens to the object after being despawned.</param>
        public void Despawn(NetworkObject nob, DespawnType? despawnType = null)
        {
            if (!IsNetworkObjectNull(true))
                _networkObjectCache.Despawn(nob, despawnType);
        }

        /// <summary>
        /// Despawns this _networkObjectCache. Can only be called on the server.
        /// </summary>
        /// <param name = "despawnType">What happens to the object after being despawned.</param>
        public void Despawn(DespawnType? despawnType = null)
        {
            if (!IsNetworkObjectNull(true))
                _networkObjectCache.Despawn(despawnType);
        }

        /// <summary>
        /// Spawns an object over the network. Can only be called on the server.
        /// </summary>
        /// <param name = "go">GameObject instance to spawn.</param>
        /// <param name = "ownerConnection">Connection to give ownership to.</param>
        public void Spawn(GameObject go, NetworkConnection ownerConnection = null, UnityEngine.SceneManagement.Scene scene = default)
        {
            if (IsNetworkObjectNull(true))
                return;
            _networkObjectCache.Spawn(go, ownerConnection, scene);
        }

        /// <summary>
        /// Spawns an object over the network. Can only be called on the server.
        /// </summary>
        /// <param name = "nob">GameObject instance to spawn.</param>
        /// <param name = "ownerConnection">Connection to give ownership to.</param>
        public void Spawn(NetworkObject nob, NetworkConnection ownerConnection = null, UnityEngine.SceneManagement.Scene scene = default)
        {
            if (IsNetworkObjectNull(true))
                return;
            _networkObjectCache.Spawn(nob, ownerConnection, scene);
        }

        /// <summary>
        /// Returns if NetworkObject is null.
        /// </summary>
        /// <param name = "warn">True to throw a warning if null.</param>
        /// <returns></returns>
        private bool IsNetworkObjectNull(bool warn)
        {
            bool isNull = _networkObjectCache == null;
            if (isNull && warn)
                NetworkManager.LogWarning($"NetworkObject is null. This can occur if this object is not spawned, or initialized yet.");

            return isNull;
        }

        /// <summary>
        /// Removes ownership from all clients.
        /// </summary>
        public void RemoveOwnership() => _networkObjectCache.RemoveOwnership();

        /// <summary>
        /// Gives ownership to newOwner.
        /// </summary>
        public void GiveOwnership(NetworkConnection newOwner) => _networkObjectCache.GiveOwnership(newOwner, asServer: true, recursive: false);

        /// <summary>
        /// Gives ownership to newOwner.
        /// </summary>
        public void GiveOwnership(NetworkConnection newOwner, bool includeNested) => _networkObjectCache.GiveOwnership(newOwner, asServer: true, includeNested);

        #region Registered components
        /// <summary>
        /// Invokes an action when a specified component becomes registered. Action will invoke immediately if already registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public void RegisterInvokeOnInstance<T>(Action<UnityEngine.Component> handler) where T : UnityEngine.Component => _networkObjectCache.RegisterInvokeOnInstance<T>(handler);

        /// <summary>
        /// Removes an action to be invoked when a specified component becomes registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public void UnregisterInvokeOnInstance<T>(Action<UnityEngine.Component> handler) where T : UnityEngine.Component => _networkObjectCache.UnregisterInvokeOnInstance<T>(handler);

        /// <summary>
        /// Returns class of type if found within CodegenBase classes.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        public T GetInstance<T>() where T : UnityEngine.Component => _networkObjectCache.GetInstance<T>();

        /// <summary>
        /// Registers a new component to this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <param name = "replace">True to replace existing references.</param>
        public void RegisterInstance<T>(T component, bool replace = true) where T : UnityEngine.Component => _networkObjectCache.RegisterInstance(component, replace);

        /// <summary>
        /// Tries to registers a new component to this NetworkManager.
        /// This will not register the instance if another already exists.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <returns>True if was able to register, false if an instance is already registered.</returns>
        public bool TryRegisterInstance<T>(T component) where T : UnityEngine.Component => _networkObjectCache.TryRegisterInstance(component);

        /// <summary>
        /// Unregisters a component from this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to unregister.</typeparam>
        public void UnregisterInstance<T>() where T : UnityEngine.Component => _networkObjectCache.UnregisterInstance<T>();
        #endregion
    }
}