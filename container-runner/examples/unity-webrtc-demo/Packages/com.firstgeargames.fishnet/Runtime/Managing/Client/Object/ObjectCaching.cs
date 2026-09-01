using FishNet.Connection;
using FishNet.Managing.Object;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.Scripting;

namespace FishNet.Managing.Client
{
    /// <summary>
    /// Information about cached network objects.
    /// </summary>
    internal class ClientObjectCache
    {
        #region Types.
        public enum CacheSearchType
        {
            Any = 0,
            Spawning = 1,
            Despawning = 2
        }
        #endregion

        #region Internal.
        /// <summary>
        /// Objects which are being spawned during iteration.
        /// </summary>
        internal Dictionary<int, NetworkObject> IteratedSpawningObjects = new();
        /// <summary>
        /// ObjectIds which have been read this tick.
        /// </summary>
        internal HashSet<int> ReadSpawningObjects = new();
        #endregion

        #region Private.
        /// <summary>
        /// Cached objects buffer. Contains spawns and despawns.
        /// </summary>
        private List<CachedNetworkObject> _cachedObjects = new();
        /// <summary>
        /// NetworkObjects which have been spawned already during the current iteration.
        /// </summary>
        private HashSet<NetworkObject> _iteratedSpawns = new();
        /// <summary>
        /// Despawns which are occurring the same tick as their spawn.
        /// </summary>
        private HashSet<int> _conflictingDespawns = new();
        /// <summary>
        /// ClientObjects reference.
        /// </summary>
        private ClientObjects _clientObjects;
        /// <summary>
        /// NetworkManager for this cache.
        /// </summary>
        private NetworkManager _networkManager;
        // /// <summary>
        // /// True if logged the warning about despawning on the same tick as the spawn.
        // /// This exist to prevent excessive spam of the warning.
        // /// </summary>
        // private bool _loggedSameTickWarning;
        /// <summary>
        /// True if initializeOrder was not default for any spawned objects.
        /// </summary>
        private bool _initializeOrderChanged;
        #endregion

        public ClientObjectCache(ClientObjects cobs, NetworkManager networkManager)
        {
            _clientObjects = cobs;
            _networkManager = networkManager;
        }

        /// <summary>
        /// Returns a NetworkObject found in spawned cache using objectId.
        /// </summary>
        /// <param name = "objectId"></param>
        /// <returns></returns>
        public NetworkObject GetInCached(int objectId, CacheSearchType searchType)
        {
            int count = _cachedObjects.Count;
            List<CachedNetworkObject> collection = _cachedObjects;
            for (int i = 0; i < count; i++)
            {
                CachedNetworkObject cnob = collection[i];
                if (cnob.ObjectId == objectId)
                {
                    // Any condition always returns.
                    if (searchType == CacheSearchType.Any)
                        return cnob.NetworkObject;

                    bool spawning = searchType == CacheSearchType.Spawning;
                    bool spawnAction = cnob.Action == CachedNetworkObject.ActionType.Spawn;
                    if (spawning == spawnAction)
                        return cnob.NetworkObject;
                    else
                        return null;
                }
            }

            // Fall through.
            return null;
        }

        /// <summary>
        /// Initializes for a spawned NetworkObject.
        /// </summary>
        public void AddSpawn(NetworkManager manager, ushort collectionId, int objectId, int initializeOrder, int ownerId, SpawnType ost, byte? nobComponentId, int? parentObjectId, byte? parentComponentId, int? prefabId, Vector3? localPosition, Quaternion? localRotation, Vector3? localScale, ulong sceneId, string sceneName, string objectName, ArraySegment<byte> payload, ArraySegment<byte> rpcLinks, ArraySegment<byte> syncValues)
        {
            // Set if initialization order has changed.
            _initializeOrderChanged |= initializeOrder != 0;

            CachedNetworkObject cnob = null;
            // If order has not changed then add normally.
            if (!_initializeOrderChanged)
            {
                cnob = ResettableObjectCaches<CachedNetworkObject>.Retrieve();
                _cachedObjects.Add(cnob);
            }
            // Otherwise see if values need to be sorted.
            else
            {
                /* Spawns will be ordered at the end of their nearest order.
                 * If spawns arrived with Id order of 5, 7, 2 then the result
                 * would be as shown below...
                 * Id 5 / order -5
                 * Id 7 / order -5
                 * Id 2 / order 0
                 * Not as if the values were inserted first such as...
                 * Id 7 / order -5
                 * Id 5 / order -5
                 * Id 2 / order 0
                 * This is to prevent the likeliness of child nobs being out of order
                 * as well to preserve user spawn order if they spawned multiple
                 * objects the same which, with the same order. */

                int written = _cachedObjects.Count;
                for (int i = 0; i < written; i++)
                {
                    CachedNetworkObject item = _cachedObjects[i];
                    /* If item order is larger then that means
                     * initializeOrder has reached the last entry
                     * of its value. Insert just before item index. */
                    if (initializeOrder < item.InitializeOrder)
                    {
                        cnob = ResettableObjectCaches<CachedNetworkObject>.Retrieve();
                        _cachedObjects.Insert(i, cnob);
                        break;
                    }
                }

                //If here and cnob is null then it was not inserted; add to end.
                if (cnob == null)
                {
                    cnob = ResettableObjectCaches<CachedNetworkObject>.Retrieve();
                    _cachedObjects.Add(cnob);
                }
            }

            cnob.InitializeSpawn(manager, collectionId, objectId, initializeOrder, ownerId, ost, nobComponentId, parentObjectId, parentComponentId, prefabId, localPosition, localRotation, localScale, sceneId, sceneName, objectName, payload, rpcLinks, syncValues);

            ReadSpawningObjects.Add(objectId);
        }

        public void AddDespawn(int objectId, DespawnType despawnType)
        {
            CachedNetworkObject cnob = ResettableObjectCaches<CachedNetworkObject>.Retrieve();
            _cachedObjects.Add(cnob);
            cnob.InitializeDespawn(objectId, despawnType);
        }

        /// <summary>
        /// Iterates any written objects.
        /// </summary>
        public void Iterate()
        {
            int written = _cachedObjects.Count;
            if (written == 0)
                return;

            try
            {
                //Indexes which have already been processed.
                HashSet<int> processedIndexes = new();
                List<CachedNetworkObject> collection = _cachedObjects;
                _conflictingDespawns.Clear();
                /* The next iteration will set rpclinks,
                 * synctypes, and so on. */
                for (int i = 0; i < written; i++)
                {
                    /* An index may already be processed if it was pushed ahead.
                     * This can occur if a nested object spawn exists but the root
                     * object has not spawned yet. In this situation the root spawn is
                     * found and performed first. */
                    if (processedIndexes.Contains(i))
                        continue;
                    CachedNetworkObject cnob = collection[i];
                    bool spawn = cnob.Action == CachedNetworkObject.ActionType.Spawn;

                    /* See if nested, and if so check if root is already spawned.
                     * If parent is not spawned then find it and process the parent first. */
                    if (spawn)
                    {
                        /* When an object is nested or has a parent it is
                         * dependent upon either the root of nested, or the parent,
                         * being spawned to setup properly.
                         *
                         * When either of these are true check spawned objects first
                         * to see if the objects exist. If not check if they are appearing
                         * later in the cache. Root or parent objects can appear later
                         * in the cache depending on the order of which observers are rebuilt.
                         * While it is possible to have the server ensure spawns always send
                         * root/parents first, that's a giant can of worms that's not worth getting into.
                         * Not only are there many scenarios to cover, but it also puts more work
                         * on the server. It's more effective to have the client handle the sorting. */

                        //Nested.
                        if (cnob.HasParent)
                        {
                            bool nested = cnob.IsInitializedNested;
                            //It's not possible to be nested and have a parent. Set the Id to look for based on if nested or parented.
                            int targetObjectId = cnob.ParentObjectId.Value;
                            NetworkObject nob = GetSpawnedObject(targetObjectId);
                            //If not spawned yet.
                            if (nob == null)
                            {
                                bool isClientHost = _networkManager.IsServerStarted;

                                bool found = false;
                                string errMsg;
                                for (int z = i + 1; z < written; z++)
                                {
                                    CachedNetworkObject zCnob = collection[z];
                                    if (zCnob.ObjectId == targetObjectId)
                                    {
                                        found = true;
                                        if (cnob.Action != CachedNetworkObject.ActionType.Spawn)
                                        {
                                            if (!isClientHost)
                                            {
                                                errMsg = nested ? $"ObjectId {targetObjectId} was found for a nested spawn, but ActionType is not spawn. ComponentIndex {cnob.ComponentId} will not be spawned." : $"ObjectId {targetObjectId} was found for a parented spawn, but ActionType is not spawn. ObjectId {cnob.ObjectId} will not be spawned.";
                                                _networkManager.LogError(errMsg);
                                            }
                                            break;
                                        }
                                        else
                                        {
                                            ProcessObject(zCnob, true, z);
                                            break;
                                        }
                                    }
                                }

                                //Root nob could not be found. Only log if not clientHost.
                                if (!found)
                                {
                                    if (!isClientHost)
                                    {
                                        errMsg = nested ? $"ObjectId {targetObjectId} could not be found for a nested spawn. ComponentIndex {cnob.ComponentId} will not be spawned." : $"ObjectId {targetObjectId} was found for a parented spawn. ObjectId {cnob.ObjectId} will not be spawned.";
                                        _networkManager.LogError(errMsg);
                                    }
                                }
                            }
                        }
                    }

                    ProcessObject(cnob, spawn, i);
                }

                void ProcessObject(CachedNetworkObject cnob, bool spawn, int index)
                {
                    processedIndexes.Add(index);

                    /* If the NetworkObject is null on lookup then something happened in the retrieval. Exit early.
                     * This can be normal on clientHost when client side gets packets late. When
                     * clientHost this will fail silently.*/

                    if (spawn)
                    {
                        if (cnob.IsSceneObject)
                        {
                            cnob.NetworkObject = _clientObjects.GetSceneNetworkObject(cnob.SceneId, cnob.SceneName, cnob.ObjectName);
                            if (cnob.NetworkObject != null)
                                SetParentAndTransformProperties(cnob);
                        }
                        //Is nested in a prefab.
                        else if (cnob.IsInitializedNested)
                        {
                            cnob.NetworkObject = _clientObjects.GetNestedNetworkObject(cnob);
                            if (cnob.NetworkObject != null)
                                cnob.NetworkObject.transform.SetLocalPositionRotationAndScale(cnob.Position, cnob.Rotation, cnob.Scale);
                        }
                        /* Not sceneObject or initializedNested. Could still be runtime
                         * nested but this also requires instantiation. The instantiation process
                         * handles parenting and position. */
                        else
                        {
                            cnob.NetworkObject = _clientObjects.GetInstantiatedNetworkObject(cnob);
                            //Parenting and transform is done during the instantiation process.
                        }
                    }
                    //Despawn.
                    else
                    {
                        cnob.NetworkObject = _clientObjects.GetSpawnedNetworkObject(cnob);

                        /* //TODO cache recent predicted despawns for up to a few seconds.
                         * If not found and the Id is in recently predicted despawned then simply
                         * do nothing. Otherwise log missing Id. */
                        // /* Do not log unless not nested. Nested nobs sometimes
                        //  * could be destroyed if parent was first. */
                        // if (!_networkManager.IsHostStarted && cnob.NetworkObject == null && !cnob.IsInitializedNested)
                        //     _networkManager.Log($"NetworkObject for ObjectId of {cnob.ObjectId} was found null. Unable to despawn object. This may occur if a nested NetworkObject had it's parent object unexpectedly destroyed. This incident is often safe to ignore.");
                    }

                    NetworkObject nob = cnob.NetworkObject;
                    //No need to error here, the other Gets above would have.
                    if (nob == null)
                        return;

                    if (spawn)
                    {
                        NetworkConnection owner;
                        int objectId;
                        //If not server then initialize by using lookups.
                        if (!_networkManager.IsServerStarted)
                        {
                            objectId = cnob.ObjectId;
                            int ownerId = cnob.OwnerId;
                            //If local client is owner then use localconnection reference.
                            NetworkConnection localConnection = _networkManager.ClientManager.Connection;
                            //If owner is self.
                            if (ownerId == localConnection.ClientId)
                            {
                                owner = localConnection;
                            }
                            else
                            {
                                /* If owner cannot be found then share owners
                                 * is disabled */
                                if (!_networkManager.ClientManager.Clients.TryGetValueIL2CPP(ownerId, out owner))
                                    owner = NetworkManager.EmptyConnection;
                            }
                        }
                        //Otherwise initialize using server values.
                        else
                        {
                            owner = nob.Owner;
                            objectId = nob.ObjectId;
                        }

                        //Preinitialize client side.
                        nob.InitializeEarly(_networkManager, objectId, owner, false);
                        //Read payload.
                        if (cnob.PayloadReader != null)
                            _networkManager.ClientManager.Objects.ReadPayload(NetworkManager.EmptyConnection, nob, cnob.PayloadReader, cnob.PayloadReader.Length);

                        _clientObjects.AddToSpawned(cnob.NetworkObject, false);
                        IteratedSpawningObjects.Add(cnob.ObjectId, cnob.NetworkObject);
                        /* Fixes https://github.com/FirstGearGames/FishNet/issues/323
                         * The redundancy may have been caused by a rework. It would seem
                         * IterateSpawn was always running after the above lines, and not
                         * from anywhere else. So there's no reason we cannot inline it
                         * here. */
                        _clientObjects.ApplyRpcLinks(cnob.NetworkObject, cnob.RpcLinkReader);
                        //IterateSpawn(cnob);
                        _iteratedSpawns.Add(cnob.NetworkObject);

                        /* Enable networkObject here if client only.
                         * This is to ensure Awake fires in the same order
                         * as InitializeOrder settings. There is no need
                         * to perform this action if server because server
                         * would have already spawned in order. */
                        if (!_networkManager.IsServerStarted && cnob.NetworkObject != null)
                            cnob.NetworkObject.gameObject.SetActive(true);
                    }
                    else
                    {
                        /* If spawned already this iteration then the nob
                         * must be initialized so that the start/stop cycles
                         * complete normally. Otherwise, the despawn callbacks will
                         * fire immediately while the start callbacks will run after all
                         * spawns have been iterated.
                         * The downside to this is that synctypes
                         * for spawns later in this iteration will not be initialized
                         * yet, and if the nob being spawned/despawned references
                         * those synctypes the values will be default.
                         *
                         * The alternative is to delay the despawning until after
                         * all spawns are iterated, but that will break the order
                         * reliability. This is unfortunately a lose/lose situation so
                         * the best we can do is let the user know the risk. */
                        NetworkObject n = cnob.NetworkObject;
                        if (_iteratedSpawns.Contains(n))
                        {
                            // if (!_loggedSameTickWarning)
                            // {
                            //     _loggedSameTickWarning = true;
                            //     _networkManager.LogWarning($"NetworkObject {cnob.NetworkObject.name} is being despawned on the same tick it's spawned." +
                            //                    $" When this occurs SyncTypes will not be set on other objects during the time of this despawn." +
                            //                    $" In result, if NetworkObject {cnob.NetworkObject.name} is referencing a SyncType of another object being spawned this tick, the returned values will be default.");
                            // }

                            _conflictingDespawns.Add(cnob.ObjectId);
                            n.gameObject.SetActive(true);
                            n.Initialize(false, true);
                        }

                        //Now being initialized, despawn the object.
                        IterateDespawn(cnob);
                    }
                }

                /* Activate the objects after all data
                 * has been synchronized. This will apply synctypes. */
                for (int i = 0; i < written; i++)
                {
                    CachedNetworkObject cnob = collection[i];
                    if (cnob.Action == CachedNetworkObject.ActionType.Spawn && cnob.NetworkObject != null)
                    {
                        _clientObjects.ApplySyncTypesForSpawn(cnob.NetworkObject, cnob.SyncTypesReader);

                        /* Only continue with the initialization if it wasn't initialized
                         * early to prevent a despawn conflict. */
                        bool canInitialize = !_conflictingDespawns.Contains(cnob.ObjectId) || !_iteratedSpawns.Contains(cnob.NetworkObject);
                        if (canInitialize)
                            cnob.NetworkObject.Initialize(false, false);
                    }
                }

                //Invoke synctype callbacks.
                for (int i = 0; i < written; i++)
                {
                    CachedNetworkObject cnob = collection[i];
                    if (cnob.Action == CachedNetworkObject.ActionType.Spawn && cnob.NetworkObject != null)
                        cnob.NetworkObject.InvokeOnStartSyncTypeCallbacks(false);
                }
            }
            finally
            {
                //Once all have been iterated reset.
                Reset();
            }
        }

        /// <summary>
        /// Sets parent using information on a CachedNetworkObject then applies transform properties.
        /// </summary>
        /// <param name = "cnob"></param>
        private void SetParentAndTransformProperties(CachedNetworkObject cnob)
        {
            if (!_networkManager.IsHostStarted && cnob.NetworkObject != null)
            {
                //Apply runtime parent if needed.
                if (cnob.HasParent)
                {
                    if (_networkManager.ClientManager.Objects.Spawned.TryGetValueIL2CPP(cnob.ParentObjectId.Value, out NetworkObject parentNob))
                    {
                        //If parented to the NOB directly.
                        if (!cnob.ParentComponentId.HasValue)
                            cnob.NetworkObject.SetParent(parentNob);
                        //Parented to a NB.
                        else
                            cnob.NetworkObject.SetParent(parentNob.NetworkBehaviours[cnob.ParentComponentId.Value]);
                    }
                    else
                    {
                        _networkManager.Log($"Parent NetworkObject Id {cnob.ParentObjectId} could not be found in spawned. NetworkObject {cnob.NetworkObject} will not have it's parent set.");
                    }

                    //cnob.NetworkObject.transform.SetLocalPositionRotationAndScale(cnob.Position, cnob.Rotation, cnob.Scale);
                }

                // else
                // {
                //     cnob.NetworkObject.transform.SetWorldPositionRotationAndScale(cnob.Position, cnob.Rotation, cnob.Scale);
                // }
                cnob.NetworkObject.transform.SetLocalPositionRotationAndScale(cnob.Position, cnob.Rotation, cnob.Scale);
            }
        }

        /// <summary>
        /// Deinitializes an object on clients and despawns the NetworkObject.
        /// </summary>
        /// <param name = "cnob"></param>
        private void IterateDespawn(CachedNetworkObject cnob)
        {
            _clientObjects.Despawn(cnob.NetworkObject, cnob.DespawnType, false);
        }

        /// <summary>
        /// Returns a NetworkObject found in spawn cache, or Spawned.
        /// </summary>
        /// <param name = "objectId"></param>
        internal NetworkObject GetSpawnedObject(int objectId)
        {
            NetworkObject result;
            //If not found in Spawning then check Spawned.
            if (!IteratedSpawningObjects.TryGetValue(objectId, out result))
            {
                IReadOnlyDictionary<int, NetworkObject> spawned = _networkManager.IsHostStarted ? _networkManager.ServerManager.Objects.Spawned : _networkManager.ClientManager.Objects.Spawned;
                spawned.TryGetValue(objectId, out result);
            }

            return result;
        }

        /// <summary>
        /// Resets cache.
        /// </summary>
        public void Reset()
        {
            _initializeOrderChanged = false;
            foreach (CachedNetworkObject item in _cachedObjects)
                ResettableObjectCaches<CachedNetworkObject>.Store(item);

            _cachedObjects.Clear();
            _iteratedSpawns.Clear();
            IteratedSpawningObjects.Clear();
            ReadSpawningObjects.Clear();
        }
    }

    /// <summary>
    /// A cached network object which exist in world but has not been Initialized yet.
    /// </summary>
    [Preserve]
    internal class CachedNetworkObject : IResettable
    {
        #region Types.
        public enum ActionType
        {
            Unset = 0,
            Spawn = 1,
            Despawn = 2
        }
        #endregion

        /// <summary>
        /// True if cached object is nested during initialization.
        /// </summary>
        public bool IsInitializedNested => ComponentId > 0;
        /// <summary>
        /// True if a scene object.
        /// </summary>
        public bool IsSceneObject => SceneId != NetworkObject.UNSET_SCENEID_VALUE;
        /// <summary>
        /// True if this object has a parent.
        /// </summary>
        public bool HasParent => ParentObjectId != null && ParentComponentId != null;
        public ushort CollectionId;
        public int ObjectId;
        public int InitializeOrder;
        public int OwnerId;
        public SpawnType SpawnType;
        public DespawnType DespawnType;
        public byte? ComponentId;
        public int? ParentObjectId;
        public byte? ParentComponentId;
        public int? PrefabId;
        public Vector3? Position;
        public Quaternion? Rotation;
        public Vector3? Scale;
        public ulong SceneId;
        public string SceneName = string.Empty;
        public string ObjectName = string.Empty;
        /// <summary>
        /// True if spawning.
        /// </summary>
        public ActionType Action { get; private set; }
        /// <summary>
        /// Cached NetworkObject.
        /// </summary>
        #pragma warning disable 0649
        public NetworkObject NetworkObject;
        /// <summary>
        /// Reader containing payload for the NetworkObject behaviours.
        /// </summary>
        public PooledReader PayloadReader;
        /// <summary>
        /// Reader containing rpc links for the NetworkObject.
        /// </summary>
        public PooledReader RpcLinkReader;
        /// <summary>
        /// Reader containing sync values for the NetworkObject.
        /// </summary>
        public PooledReader SyncTypesReader;
        #pragma warning restore 0649

        public void InitializeSpawn(NetworkManager manager, ushort collectionId, int objectId, int initializeOrder, int ownerId, SpawnType objectSpawnType, byte? nobComponentId, int? parentObjectId, byte? parentComponentId, int? prefabId, Vector3? position, Quaternion? rotation, Vector3? scale, ulong sceneId, string sceneName, string objectName, ArraySegment<byte> payload, ArraySegment<byte> rpcLinks, ArraySegment<byte> syncTypes)
        {
            ResetState();
            Action = ActionType.Spawn;
            CollectionId = collectionId;
            ObjectId = objectId;
            InitializeOrder = initializeOrder;
            OwnerId = ownerId;
            SpawnType = objectSpawnType;
            ComponentId = nobComponentId;
            ParentObjectId = parentObjectId;
            ParentComponentId = parentComponentId;
            PrefabId = prefabId;
            Position = position;
            Rotation = rotation;
            Scale = scale;
            SceneId = sceneId;
            SceneName = sceneName;
            ObjectName = objectName;

            if (payload.Count > 0)
                PayloadReader = ReaderPool.Retrieve(payload, manager);
            if (rpcLinks.Count > 0)
                RpcLinkReader = ReaderPool.Retrieve(rpcLinks, manager);
            if (syncTypes.Count > 0)
                SyncTypesReader = ReaderPool.Retrieve(syncTypes, manager);
        }

        /// <summary>
        /// Initializes for a despawned NetworkObject.
        /// </summary>
        /// <param name = "nob"></param>
        public void InitializeDespawn(int objectId, DespawnType despawnType)
        {
            ResetState();
            Action = ActionType.Despawn;
            DespawnType = despawnType;
            ObjectId = objectId;
        }

        /// <summary>
        /// Resets values which could malform identify the cached object.
        /// </summary>
        public void ResetState()
        {
            SceneName = string.Empty;
            ObjectName = string.Empty;
            NetworkObject = null;

            ReaderPool.StoreAndDefault(ref PayloadReader);
            ReaderPool.StoreAndDefault(ref RpcLinkReader);
            ReaderPool.StoreAndDefault(ref SyncTypesReader);
        }

        public void InitializeState() { }

        ~CachedNetworkObject()
        {
            NetworkObject = null;
        }
    }
}