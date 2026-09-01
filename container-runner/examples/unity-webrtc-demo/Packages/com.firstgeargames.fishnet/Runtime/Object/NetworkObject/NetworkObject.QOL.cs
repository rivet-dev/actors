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
using System;
using System.Collections.Generic;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        #region Public.
        #region Obsoletes
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
        /// True if despawning without object pooling, or if OnDestroy was invoked on this NetworkObject. As clientHost this value becomes true when previous criteria are met and server begins to deinitialize the object.
        /// </summary>
        /// <remarks>This can be useful for checking if you wish to perform certain actions within OnStopNetwork based on destroying status.</remarks>
        public bool IsDestroying { get; private set; }

        /// <summary>
        /// Sets IsDestroying to true if DespawnType is not pooled. When DespawnType is not specified default DespawnType is checked.
        /// </summary>
        internal void SetIsDestroying(DespawnType? despawnType = null)
        {
            if (despawnType.HasValue)
            {
                if (despawnType.Value == DespawnType.Destroy)
                    IsDestroying = true;
            }
            else if (GetDefaultDespawnType() == DespawnType.Destroy)
            {
                IsDestroying = true;
            }
        }

        /// <summary>
        /// True if predicted spawning is allowed for this object.
        /// </summary>
        internal bool AllowPredictedSpawning => PredictedSpawn == null ? false : PredictedSpawn.GetAllowSpawning();
        /// <summary>
        /// True if predicted spawning is allowed for this object.
        /// </summary>
        internal bool AllowPredictedDespawning => PredictedSpawn == null ? false : PredictedSpawn.GetAllowDespawning();
        /// <summary>
        /// True if this object has been initialized on the client side.
        /// This is set true right before client start callbacks and after stop callbacks.
        /// </summary>
        public bool IsClientInitialized { get; private set; }
        /// <summary>
        /// True if the client is started and authenticated. This will return true on clientHost even if the object has not initialized yet for the client.
        /// To check if this object has been initialized for the client use IsClientInitialized.
        /// </summary>
        public bool IsClientStarted => NetworkManager == null ? false : NetworkManager.IsClientStarted;
        /// <summary>
        /// True if this object has been initialized only on the server side.
        /// This is set true right before server start callbacks and after stop callbacks.
        /// </summary>
        public bool IsClientOnlyInitialized => !IsServerInitialized && IsClientInitialized;
        /// <summary>
        /// True if only the client is started and authenticated.
        /// </summary>
        public bool IsClientOnlyStarted => IsClientStarted && !IsServerStarted;
        /// <summary>
        /// True if this object has been initialized on the server side.
        /// This is set true right before server start callbacks and after stop callbacks.
        /// </summary>
        public bool IsServerInitialized { get; private set; }
        /// <summary>
        /// True if the server is active. This will return true on clientHost even if the object is being deinitialized on the server.
        /// To check if this object has been initialized for the server use IsServerInitialized.
        /// </summary>
        public bool IsServerStarted => NetworkManager == null ? false : NetworkManager.IsServerStarted;
        /// <summary>
        /// True if this object has been initialized only on the server side.
        /// This is set true right before server start callbacks and after stop callbacks.
        /// </summary>
        public bool IsServerOnlyInitialized => IsServerInitialized && !IsClientInitialized;
        /// <summary>
        /// True if only the server is started.
        /// </summary>
        public bool IsServerOnlyStarted => IsServerStarted && !IsClientStarted;
        /// <summary>
        /// True if client and server are started.
        /// </summary>
        public bool IsHostStarted => IsClientStarted && IsServerStarted;
        /// <summary>
        /// True if this object has been initialized on the server and client side.
        /// </summary>
        public bool IsHostInitialized => IsClientInitialized && IsServerInitialized;
        /// <summary>
        /// True if client nor server are started.
        /// </summary>
        public bool IsOffline => !IsClientStarted && !IsServerStarted;
        /// <summary>
        /// True if a reconcile is occuring on the PredictionManager. Note the difference between this and IsBehaviourReconciling.
        /// </summary>
        public bool IsManagerReconciling => PredictionManager.IsReconciling;
        /// <summary>
        /// True if the local client is currently using a PredictedOwner component on this object to take ownership.
        /// </summary>
        public bool IsTakingOwnership => PredictedOwner != null && PredictedOwner.TakingOwnership;
        /// <summary>
        /// True if the local client is the owner of this object.
        /// This will only return true if IsClientInitialized is also true. You may check ownership status regardless of client initialized state by using Owner.IsLocalClient.
        /// </summary>
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartServer", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartNetwork", " Use base.Owner.IsLocalClient instead.")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Awake", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Start", "")]
        public bool IsOwner
        {
            get
            {
                /* ClientInitialized becomes true when this
                 * NetworkObject has been initialized on the client side.
                 *
                 * This value is used to prevent IsOwner from returning true
                 * when running as host; primarily in Update or Tick callbacks
                 * where IsOwner would be true as host but OnStartClient has
                 * not called yet.
                 *
                 * EG: server will set owner when it spawns the object.
                 * If IsOwner is checked before the object spawns on the
                 * client-host then it would also return true, since the
                 * Owner reference would be the same as what was set by server.
                 *
                 * This is however bad when the client hasn't initialized the object
                 * yet because it gives a false sense of execution order.
                 * As a result, Update or Ticks may return IsOwner as true well before OnStartClient
                 * is called. Many users rightfully create code with the assumption the client has been
                 * initialized by the time IsOwner is true.
                 *
                 * This is a double edged sword though because now IsOwner would return true
                 * within OnStartNetwork for clients only, but not for host given the client
                 * side won't be initialized yet as host. As a work around CodeAnalysis will
                 * inform users to instead use base.Owner.IsLocalClient within OnStartNetwork. */
                if (!IsClientInitialized)
                    return false;

                return Owner.IsLocalClient;
            }
        }
        /// <summary>
        /// True if IsOwner, or if IsServerInitialized with no Owner.
        /// </summary>
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartServer", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "OnStartNetwork", " Use (base.Owner.IsLocalClient || (base.IsServerInitialized && !Owner.Isvalid) instead.")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Awake", "")]
        [PreventUsageInside("global::FishNet.Object.NetworkBehaviour", "Start", "")]
        public bool IsController => IsOwner || (IsServerInitialized && !Owner.IsValid);
        [Obsolete("Use IsController.")]
        public bool HasAuthority => IsController;
        /// <summary>
        /// </summary>
        private NetworkConnection _owner;
        /// <summary>
        /// Owner of this object.
        /// </summary>
        public NetworkConnection Owner
        {
            get
            {
                // Ensures a null Owner is never returned.
                if (_owner == null)
                    return NetworkManager.EmptyConnection;

                return _owner;
            }
            private set { _owner = value; }
        }
        /// <summary>
        /// ClientId for this NetworkObject owner.
        /// </summary>
        public int OwnerId => !Owner.IsValid ? -1 : Owner.ClientId;
        /// <summary>
        /// True if the object is initialized for the network.
        /// </summary>
        public bool IsSpawned => !IsDeinitializing && ObjectId != UNSET_OBJECTID_VALUE;
        /// <summary>
        /// The local connection of the client calling this method.
        /// </summary>
        public NetworkConnection LocalConnection => NetworkManager == null ? new() : NetworkManager.ClientManager.Connection;
        /// <summary>
        /// NetworkManager for this object.
        /// </summary>
        public NetworkManager NetworkManager { get; private set; }
        /// <summary>
        /// ServerManager for this object.
        /// </summary>
        public ServerManager ServerManager { get; private set; }
        /// <summary>
        /// ClientManager for this object.
        /// </summary>
        public ClientManager ClientManager { get; private set; }
        /// <summary>
        /// ObserverManager for this object.
        /// </summary>
        public ObserverManager ObserverManager { get; private set; }
        /// <summary>
        /// TransportManager for this object.
        /// </summary>
        public TransportManager TransportManager { get; private set; }
        /// <summary>
        /// TimeManager for this object.
        /// </summary>
        public TimeManager TimeManager { get; private set; }
        /// <summary>
        /// SceneManager for this object.
        /// </summary>
        public SceneManager SceneManager { get; private set; }
        /// <summary>
        /// PredictionManager for this object.
        /// </summary>
        public PredictionManager PredictionManager { get; private set; }
        /// <summary>
        /// RollbackManager for this object.
        /// </summary>
        public RollbackManager RollbackManager { get; private set; }
        #endregion

        /// <summary>
        /// Returns a NetworkBehaviour on this NetworkObject.
        /// </summary>
        /// <param name = "componentIndex">ComponentIndex of the NetworkBehaviour.</param>
        /// <param name = "error">True to error if not found.</param>
        /// <returns></returns>
        public NetworkBehaviour GetNetworkBehaviour(byte componentIndex, bool error)
        {
            if (componentIndex >= NetworkBehaviours.Count)
            {
                if (error)
                {
                    string message = $"ComponentIndex of {componentIndex} is out of bounds on {gameObject.name} [id {ObjectId}]. This may occur if you have modified your gameObject/prefab without saving it, or the scene.";
                    NetworkManager.LogError(message);
                }
            }

            return NetworkBehaviours[componentIndex];
        }

        /// <summary>
        /// Despawns a GameObject. Only call from the server.
        /// </summary>
        /// <param name = "go">GameObject to despawn.</param>
        /// <param name = "despawnType">What happens to the object after being despawned.</param>
        public void Despawn(GameObject go, DespawnType? despawnType = null)
        {
            if (NetworkManager != null)
                NetworkManager.ServerManager.Despawn(go, despawnType);
        }

        /// <summary>
        /// Despawns  a NetworkObject. Only call from the server.
        /// </summary>
        /// <param name = "nob">NetworkObject to despawn.</param>
        /// <param name = "despawnType">What happens to the object after being despawned.</param>
        public void Despawn(NetworkObject nob, DespawnType? despawnType = null)
        {
            if (NetworkManager != null)
                NetworkManager.ServerManager.Despawn(nob, despawnType);
        }

        /// <summary>
        /// Despawns this NetworkObject. Only call from the server.
        /// </summary>
        /// <param name = "despawnType">What happens to the object after being despawned.</param>
        public void Despawn(DespawnType? despawnType = null)
        {
            NetworkObject nob = this;
            if (NetworkManager != null)
                NetworkManager.ServerManager.Despawn(nob, despawnType);
        }

        /// <summary>
        /// Spawns an object over the network. Only call from the server.
        /// </summary>
        public void Spawn(GameObject go, NetworkConnection ownerConnection = null, UnityEngine.SceneManagement.Scene scene = default)
        {
            if (NetworkManager != null)
                NetworkManager.ServerManager.Spawn(go, ownerConnection, scene);
        }

        /// <summary>
        /// Spawns an object over the network. Only call from the server.
        /// </summary>
        public void Spawn(NetworkObject nob, NetworkConnection ownerConnection = null, UnityEngine.SceneManagement.Scene scene = default)
        {
            if (NetworkManager != null)
                NetworkManager.ServerManager.Spawn(nob, ownerConnection, scene);
        }

        [Obsolete("Use SetLocalOwnership(NetworkConnection, bool).")]
        public void SetLocalOwnership(NetworkConnection caller) => SetLocalOwnership(caller, recursive: false);

        /// <summary>
        /// Takes ownership of this object and child network objects, allowing immediate control.
        /// </summary>
        /// <param name = "caller">Connection to give ownership to.</param>
        public void SetLocalOwnership(NetworkConnection caller, bool recursive)
        {
            NetworkConnection prevOwner = Owner;
            SetOwner(caller);

            int count;
            count = NetworkBehaviours.Count;
            for (int i = 0; i < count; i++)
                NetworkBehaviours[i].OnOwnershipClient_Internal(prevOwner);

            if (recursive)
            {
                List<NetworkObject> allNested = GetNetworkObjects(GetNetworkObjectOption.AllNestedRecursive);

                foreach (NetworkObject nob in allNested)
                    nob.SetLocalOwnership(caller, recursive: true);

                CollectionCaches<NetworkObject>.Store(allNested);
            }
        }

        #region Registered components
        /// <summary>
        /// Invokes an action when a specified component becomes registered. Action will invoke immediately if already registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public void RegisterInvokeOnInstance<T>(Action<UnityEngine.Component> handler) where T : UnityEngine.Component => NetworkManager.RegisterInvokeOnInstance<T>(handler);

        /// <summary>
        /// Removes an action to be invoked when a specified component becomes registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public void UnregisterInvokeOnInstance<T>(Action<UnityEngine.Component> handler) where T : UnityEngine.Component => NetworkManager.UnregisterInvokeOnInstance<T>(handler);

        /// <summary>
        /// Returns if an instance exists for type.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        public bool HasInstance<T>() where T : UnityEngine.Component => NetworkManager.HasInstance<T>();

        /// <summary>
        /// Returns class of type if found within CodegenBase classes.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        public T GetInstance<T>() where T : UnityEngine.Component => NetworkManager.GetInstance<T>();

        /// <summary>
        /// Registers a new component to this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <param name = "replace">True to replace existing references.</param>
        public void RegisterInstance<T>(T component, bool replace = true) where T : UnityEngine.Component => NetworkManager.RegisterInstance(component, replace);

        /// <summary>
        /// Tries to registers a new component to this NetworkManager.
        /// This will not register the instance if another already exists.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <returns>True if was able to register, false if an instance is already registered.</returns>
        public bool TryRegisterInstance<T>(T component) where T : UnityEngine.Component => NetworkManager.TryRegisterInstance(component);

        /// <summary>
        /// Returns class of type from registered instances.
        /// </summary>
        /// <param name = "component">Outputted component.</param>
        /// <typeparam name = "T">Type to get.</typeparam>
        /// <returns>True if was able to get instance.</returns>
        public bool TryGetInstance<T>(out T component) where T : UnityEngine.Component => NetworkManager.TryGetInstance(out component);

        /// <summary>
        /// Unregisters a component from this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to unregister.</typeparam>
        public void UnregisterInstance<T>() where T : UnityEngine.Component => NetworkManager.UnregisterInstance<T>();
        #endregion
    }
}