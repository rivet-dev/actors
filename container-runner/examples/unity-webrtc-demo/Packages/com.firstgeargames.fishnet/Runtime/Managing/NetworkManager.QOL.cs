using FishNet.Managing.Object;
using FishNet.Object;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine;
using UnityComponent = UnityEngine.Component;

namespace FishNet.Managing
{
    public partial class NetworkManager : MonoBehaviour
    {
        #region Public.
        #region Obsoletes
        [Obsolete("Use IsClientOnlyStarted. Note the difference between IsClientOnlyInitialized and IsClientOnlyStarted.")]
        public bool IsClientOnly => IsClientOnlyStarted;
        [Obsolete("Use IsServerOnlyStarted. Note the difference between IsServerOnlyInitialized and IsServerOnlyStarted.")]
        public bool IsServerOnly => IsServerOnlyStarted;
        [Obsolete("Use IsHostStarted. Note the difference between IsHostInitialized and IsHostStarted.")]
        public bool IsHost => IsHostStarted;
        [Obsolete("Use IsClientStarted. Note the difference between IsClientInitialized and IsClientStarted.")]
        public bool IsClient => IsClientStarted;
        [Obsolete("Use IsServerStarted. Note the difference between IsServerInitialized and IsServerStarted.")]
        public bool IsServer => IsServerStarted;
        #endregion

        /// <summary>
        /// True if server is started.
        /// </summary>
        public bool IsServerStarted => ServerManager.Started;
        /// <summary>
        /// True if only the server is started.
        /// </summary>
        public bool IsServerOnlyStarted => IsServerStarted && !IsClientStarted;
        /// <summary>
        /// True if the client is authenticated.
        /// </summary>
        public bool IsClientStarted => ClientManager.Started && ClientManager.Connection.IsAuthenticated;
        /// <summary>
        /// True if only the client is authenticated.
        /// </summary>
        public bool IsClientOnlyStarted => !IsServerStarted && IsClientStarted;
        /// <summary>
        /// True if client is authenticated, and the server is started.
        /// </summary>
        public bool IsHostStarted => IsServerStarted && IsClientStarted;
        /// <summary>
        /// True if client nor server are started.
        /// </summary>
        public bool IsOffline => !IsServerStarted && !IsClientStarted;
        #endregion

        #region Serialized.
        /// <summary>
        /// </summary>
        [Tooltip("Collection to use for spawnable objects.")]
        [SerializeField]
        private PrefabObjects _spawnablePrefabs;
        /// <summary>
        /// Collection to use for spawnable objects.
        /// </summary>
        public PrefabObjects SpawnablePrefabs
        {
            get => _spawnablePrefabs;
            set => _spawnablePrefabs = value;
        }
        /// <summary>
        /// </summary>
        private Dictionary<ushort, PrefabObjects> _runtimeSpawnablePrefabs = new();
        /// <summary>
        /// Collection to use for spawnable objects added at runtime, such as addressables.
        /// </summary>
        public IReadOnlyDictionary<ushort, PrefabObjects> RuntimeSpawnablePrefabs => _runtimeSpawnablePrefabs;
        #endregion

        #region Private.
        /// <summary>
        /// Delegates waiting to be invoked when a component is registered.
        /// </summary>
        private Dictionary<string, List<Action<UnityComponent>>> _pendingInvokes = new();
        /// <summary>
        /// Currently registered components.
        /// </summary>
        private Dictionary<string, UnityComponent> _registeredComponents = new();
        #endregion

        /// <summary>
        /// Gets the PrefabObjects to use for spawnableCollectionId.
        /// </summary>
        /// <typeparam name = "T">Type of PrefabObjects to return. This is also used to create an instance of type when createIfMissing is true.</typeparam>
        /// <param name = "spawnableCollectionId">Id to use. 0 will return the configured SpawnablePrefabs.</param>
        /// <param name = "createIfMissing">True to create and assign a PrefabObjects if missing for the collectionId.</param>
        /// <returns></returns>
        public PrefabObjects GetPrefabObjects<T>(ushort spawnableCollectionId, bool createIfMissing) where T : PrefabObjects
        {
            if (spawnableCollectionId == 0)
            {
                if (createIfMissing)
                {
                    InternalLogError($"SpawnableCollectionId cannot be 0 when create missing is true.");
                    return null;
                }
                else
                {
                    return SpawnablePrefabs;
                }
            }

            PrefabObjects po;
            if (!_runtimeSpawnablePrefabs.TryGetValue(spawnableCollectionId, out po))
            {
                // Do not create missing, return null for not found.
                if (!createIfMissing)
                    return null;

                po = ScriptableObject.CreateInstance<T>();
                po.SetCollectionId(spawnableCollectionId);
                _runtimeSpawnablePrefabs[spawnableCollectionId] = po;
            }

            return po;
        }

        /// <summary>
        /// Removes the PrefabObjects collection from memory.
        /// This should only be called after you properly disposed of it's contents properly.
        /// </summary>
        /// <param name = "spawnableCollectionId">CollectionId to remove.</param>
        /// <returns>True if collection was found and removed.</returns>
        public bool RemoveSpawnableCollection(ushort spawnableCollectionId)
        {
            return _runtimeSpawnablePrefabs.Remove(spawnableCollectionId);
        }

        /// <summary>
        /// Gets the index a prefab uses. Can be used in conjuction with GetPrefab.
        /// </summary>
        /// <param name = "prefab"></param>
        /// <param name = "asServer">True if to get from the server collection.</param>
        /// <returns>Returns index if found, and -1 if not found.</returns>
        public int GetPrefabIndex(GameObject prefab, bool asServer)
        {
            int count = SpawnablePrefabs.GetObjectCount();
            for (int i = 0; i < count; i++)
            {
                GameObject go = SpawnablePrefabs.GetObject(asServer, i).gameObject;
                if (go == prefab)
                    return i;
            }

            // Fall through, not found.
            return -1;
        }

        /// <summary>
        /// Returns a prefab with prefabId.
        /// This method will bypass object pooling.
        /// </summary>
        /// <param name = "prefabId">PrefabId to get.</param>
        /// <param name = "asServer">True if getting the prefab asServer.</param>
        public NetworkObject GetPrefab(int prefabId, bool asServer)
        {
            return SpawnablePrefabs.GetObject(asServer, prefabId);
        }

        #region Registered components
        /// <summary>
        /// Invokes an action when a specified component becomes registered. Action will invoke immediately if already registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public void RegisterInvokeOnInstance<T>(Action<UnityComponent> handler) where T : UnityComponent
        {
            T result;
            // If not found yet make a pending invoke.
            if (!TryGetInstance(out result))
            {
                string tName = GetInstanceName<T>();
                List<Action<UnityComponent>> handlers;
                if (!_pendingInvokes.TryGetValue(tName, out handlers))
                {
                    handlers = new();
                    _pendingInvokes[tName] = handlers;
                }

                handlers.Add(handler);
            }
            // Already exist, invoke right away.
            else
            {
                handler.Invoke(result);
            }
        }

        /// <summary>
        /// Removes an action to be invokes when a specified component becomes registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public void UnregisterInvokeOnInstance<T>(Action<UnityComponent> handler) where T : UnityComponent
        {
            string tName = GetInstanceName<T>();
            List<Action<UnityComponent>> handlers;
            if (!_pendingInvokes.TryGetValue(tName, out handlers))
                return;

            handlers.Remove(handler);
            // Do not remove pending to prevent garbage collection later from recreation.
        }

        /// <summary>
        /// Returns if an instance exists for type.
        /// </summary>
        /// <typeparam name = "T">Type to check.</typeparam>
        /// <returns></returns>
        public bool HasInstance<T>() where T : UnityComponent
        {
            return TryGetInstance<T>(out _);
        }

        /// <summary>
        /// Returns class of type from registered instances.
        /// A warning will display if not found.
        /// </summary>
        /// <typeparam name = "T">Type to get.</typeparam>
        /// <returns></returns>
        public T GetInstance<T>() where T : UnityComponent
        {
            T result;
            if (TryGetInstance(out result))
                return result;
            else
                InternalLogWarning($"Component {GetInstanceName<T>()} is not registered. To avoid this warning use TryGetInstance(T).");

            return default;
        }

        /// <summary>
        /// Returns class of type from registered instances.
        /// </summary>
        /// <param name = "component">Outputted component.</param>
        /// <typeparam name = "T">Type to get.</typeparam>
        /// <returns>True if was able to get instance.</returns>
        public bool TryGetInstance<T>(out T result) where T : UnityComponent
        {
            string tName = GetInstanceName<T>();
            if (_registeredComponents.TryGetValue(tName, out UnityComponent v))
            {
                result = (T)v;
                return true;
            }
            else
            {
                result = default;
                return false;
            }
        }

        /// <summary>
        /// Registers a new component to this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <param name = "replace">True to replace existing references.</param>
        public void RegisterInstance<T>(T component, bool replace = true) where T : UnityComponent
        {
            string tName = GetInstanceName<T>();
            if (_registeredComponents.ContainsKey(tName) && !replace)
            {
                InternalLogWarning($"Component {tName} is already registered.");
            }
            else
            {
                _registeredComponents[tName] = component;
                RemoveNullPendingDelegates();
                // If in pending invokes also send these out.
                if (_pendingInvokes.TryGetValue(tName, out List<Action<UnityComponent>> dels))
                {
                    for (int i = 0; i < dels.Count; i++)
                        dels[i].Invoke(component);
                    /* Clear delegates but do not remove dictionary entry
                     * to prevent list from being re-initialized. */
                    dels.Clear();
                }
            }
        }

        /// <summary>
        /// Tries to registers a new component to this NetworkManager.
        /// This will not register the instance if another already exists.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <returns>True if was able to register, false if an instance is already registered.</returns>
        public bool TryRegisterInstance<T>(T component) where T : UnityComponent
        {
            string tName = GetInstanceName<T>();
            if (_registeredComponents.ContainsKey(tName))
                return false;
            else
                RegisterInstance(component, false);

            return true;
        }

        /// <summary>
        /// Unregisters a component from this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to unregister.</typeparam>
        public void UnregisterInstance<T>() where T : UnityComponent
        {
            string tName = GetInstanceName<T>();
            _registeredComponents.Remove(tName);
        }

        /// <summary>
        /// Removes delegates from pending invokes when may have gone missing.
        /// </summary>
        private void RemoveNullPendingDelegates()
        {
            foreach (List<Action<UnityComponent>> handlers in _pendingInvokes.Values)
            {
                for (int i = 0; i < handlers.Count; i++)
                {
                    if (handlers[i] == null)
                    {
                        handlers.RemoveAt(i);
                        i--;
                    }
                }
            }
        }

        /// <summary>
        /// Returns the name to use for T.
        /// </summary>
        private string GetInstanceName<T>()
        {
            return typeof(T).FullName;
        }
        #endregion
    }
}