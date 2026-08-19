#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Connection;
using FishNet.Managing.Object;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using GameKit.Dependencies.Utilities.Types;
using System;
using System.Collections.Generic;
using FishNet.Managing.Logging;
using FishNet.Object.Synchronizing;
using FishNet.Serializing.Helping;
using FishNet.Utility.Performance;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace FishNet.Managing.Server
{
    /// <summary>
    /// Handles objects and information about objects for the server. See ManagedObjects for inherited options.
    /// </summary>
    public partial class ServerObjects : ManagedObjects
    {
        #region Public.
        /// <summary>
        /// Called right before client objects are destroyed when a client disconnects.
        /// </summary>
        public event Action<NetworkConnection> OnPreDestroyClientObjects;
        #endregion

        #region Internal.
        /// <summary>
        /// Collection of NetworkObjects recently despawned.
        /// Key: objectId.
        /// Value: despawn tick.
        /// This is used primarily to track if client is sending messages for recently despawned objects.
        /// Objects are automatically removed after RECENTLY_DESPAWNED_DURATION seconds.
        /// </summary>
        internal Dictionary<int, uint> RecentlyDespawnedIds = new();
        #endregion

        #region Private.
        /// <summary>
        /// Cached ObjectIds which may be used when exceeding available ObjectIds.
        /// </summary>
        private Queue<int> _objectIdCache = new();

        /// <summary>
        /// Returns the ObjectId cache.
        /// </summary>
        /// <returns></returns>
        internal Queue<int> GetObjectIdCache() => _objectIdCache;

        /// <summary>
        /// NetworkBehaviours which have dirty SyncObjects.
        /// </summary>
        private List<NetworkBehaviour> _dirtySyncTypeBehaviours = new(20);
        /// <summary>
        /// Objects which need to be destroyed next tick.
        /// This is needed when running as host so host client will get any final messages for the object before they're destroyed.
        /// </summary>
        private HashSet<NetworkObject> _pendingDestroy = new();
        /// <summary>
        /// NetworkObjects in a recently loaded scene.
        /// </summary>
        private List<(int, List<NetworkObject>)> _loadedSceneNetworkObjects = new List<(int frame, List<NetworkObject> nobs)>();
        /// <summary>
        /// Cache of spawning objects, used for recursively spawning nested NetworkObjects.
        /// </summary>
        private List<NetworkObject> _spawnCache = new();
        /// <summary>
        /// True if one or more scenes are currently loading through the SceneManager.
        /// </summary>
        private bool _scenesLoading;
        /// <summary>
        /// Number of ticks which must pass to clear a recently despawned.
        /// </summary>
        private uint _cleanRecentlyDespawnedMaxTicks => NetworkManager.TimeManager.TimeToTicks(30d, TickRounding.RoundUp);
        #endregion

        internal ServerObjects(NetworkManager networkManager)
        {
            base.Initialize(networkManager);
            networkManager.SceneManager.OnLoadStart += SceneManager_OnLoadStart;
            networkManager.SceneManager.OnActiveSceneSetInternal += SceneManager_OnActiveSceneSet;
            networkManager.TimeManager.OnUpdate += TimeManager_OnUpdate;
        }

        /// <summary>
        /// Called when MonoBehaviours call Update.
        /// </summary>
        private void TimeManager_OnUpdate()
        {
            if (!NetworkManager.IsServerStarted)
            {
                _scenesLoading = false;
                ClearSceneLoadedNetworkObjects();
                return;
            }

            CleanRecentlyDespawned();

            if (!_scenesLoading)
                IterateLoadedScenes(false);

            Observers_OnUpdate();
        }

        /// <summary>
        /// Clears NetworkObjects pending initialization from a recently loaded scene.
        /// </summary>
        private void ClearSceneLoadedNetworkObjects()
        {
            for (int i = 0; i < _loadedSceneNetworkObjects.Count; i++)
            {
                (int frame, List<NetworkObject> nobs) value = _loadedSceneNetworkObjects[i];
                CollectionCaches<NetworkObject>.Store(value.nobs);
            }

            _loadedSceneNetworkObjects.Clear();
        }

        #region Checking dirty SyncTypes.
        /// <summary>
        /// Iterates NetworkBehaviours with dirty SyncTypes.
        /// </summary>
        internal void WriteDirtySyncTypes()
        {
            List<NetworkBehaviour> collection = _dirtySyncTypeBehaviours;
            int colStart = collection.Count;
            if (colStart == 0)
                return;

            /* Tells networkbehaviours to check their
             * dirty synctypes. */
            for (int i = 0; i < collection.Count; i++)
            {
                bool dirtyCleared = collection[i].WriteDirtySyncTypes(SyncTypeWriteFlag.Unset);
                if (dirtyCleared)
                {
                    collection.RemoveAt(i);
                    i--;
                }
            }
        }

        /// <summary>
        /// Sets that a NetworkBehaviour has a dirty syncVars.
        /// </summary>
        internal void SetDirtySyncType(NetworkBehaviour nb)
        {
            _dirtySyncTypeBehaviours.Add(nb);
        }
        #endregion

        #region Connection Handling.
        /// <summary>
        /// Called when the connection state changes for the local server.
        /// </summary>
        /// <param name = "args"></param>
        internal void OnServerConnectionState(ServerConnectionStateArgs args)
        {
            // If server just connected.
            if (args.ConnectionState == LocalConnectionState.Started)
            {
                /* If there's no servers started besides the one
                 * that just started then build Ids and setup scene objects. */
                if (NetworkManager.ServerManager.IsOnlyOneServerStarted())
                {
                    BuildObjectIdCache();
                    SetupSceneObjects();
                }
            }
            // Server in anything but started state.
            else
            {
                // If no servers are started then reset.
                if (!NetworkManager.ServerManager.IsAnyServerStarted())
                {
                    base.DespawnWithoutSynchronization(recursive: true, asServer: true);
                    SceneObjects_Internal.Clear();
                    _objectIdCache.Clear();
                    NetworkManager.ClearClientsCollection(NetworkManager.ServerManager.Clients);
                }
                // If at least one server is started then only clear for disconnecting server.
                else
                {
                    int transportIndex = args.TransportIndex;
                    // Remove connection from all NetworkObjects to ensure they are not stuck in observers.
                    foreach (NetworkConnection c in NetworkManager.ServerManager.Clients.Values)
                    {
                        if (c.TransportIndex == transportIndex)
                            RemoveFromObserversWithoutSynchronization(c);
                    }

                    // Remove connections only for transportIndex.
                    NetworkManager.ClearClientsCollection(NetworkManager.ServerManager.Clients, transportIndex);
                }
            }
        }

        /// <summary>
        /// Called when a client disconnects.
        /// </summary>
        /// <param name = "connection"></param>
        internal void ClientDisconnected(NetworkConnection connection)
        {
            RemoveFromObserversWithoutSynchronization(connection);

            if (OnPreDestroyClientObjects != null)
                OnPreDestroyClientObjects.Invoke(connection);

            /* A cache is made because the Objects
             * collection would end up modified during
             * iteration from removing ownership and despawning. */
            List<NetworkObject> nobs = CollectionCaches<NetworkObject>.RetrieveList();
            foreach (NetworkObject nob in connection.Objects)
                nobs.Add(nob);

            int nobsCount = nobs.Count;
            for (int i = 0; i < nobsCount; i++)
            {
                NetworkObject n = nobs[i];
                /* Objects may already be deinitializing when a client disconnects
                 * because the root object could have been despawned first, and in result
                 * all child objects would have been recursively despawned.
                 *
                 * EG: object is:
                 *      A (nob)
                 *          B (nob)
                 *
                 * Both A and B are owned by the client so they will both be
                 * in collection. Should A despawn first B will recursively despawn
                 * from it. Then once that finishes and the next index of collection
                 * is run, which would B, the object B would have already been deinitialized. */
                if (!n.IsDeinitializing && !n.PreventDespawnOnDisconnect)
                    NetworkManager.ServerManager.Despawn(nobs[i]);
            }

            CollectionCaches<NetworkObject>.Store(nobs);
        }
        #endregion

        #region ObjectIds.
        /// <summary>
        /// Builds the ObjectId cache with all possible Ids.
        /// </summary>
        private void BuildObjectIdCache()
        {
            _objectIdCache.Clear();

            /* Shuffle Ids to make it more difficult
             * for clients to track spawned object
             * count. */
            List<int> shuffledCache = new();
            // Ignore ushort.maxvalue as that indicates null.
            for (int i = 0; i < ushort.MaxValue - 1; i++)
                // for (int i = 0; i < (2200); i++) // QUICK-TEST Uncomment this, and comment the line above.
                shuffledCache.Add(i);
            /* Only shuffle when NOT in editor and not
             * development build.
             * Debugging could be easier when Ids are ordered. */
#if !DEVELOPMENT
            shuffledCache.Shuffle();
#endif
            // Add shuffled to objectIdCache.
            // Build Id cache.
            int cacheCount = shuffledCache.Count;
            for (int i = 0; i < cacheCount; i++)
                _objectIdCache.Enqueue(shuffledCache[i]);
        }

        /// <summary>
        /// Caches a NetworkObject ObjectId.
        /// </summary>
        private void CacheObjectId(NetworkObject nob)
        {
            if (nob.ObjectId != NetworkObject.UNSET_OBJECTID_VALUE)
                CacheObjectId(nob.ObjectId);
        }

        /// <summary>
        /// Adds an ObjectId to objectId cache.
        /// </summary>
        /// <param name = "id"></param>
        internal void CacheObjectId(int id)
        {
            if (!_objectIdCache.Contains(id))
                _objectIdCache.Enqueue(id);
            else
                NetworkManager.LogError($"Object Id [{id}] already exists within ObjectId Cache. Please report this error.");
        }

        /// <summary>
        /// Gets the next ObjectId to use for NetworkObjects.
        /// </summary>
        /// <returns></returns>
        protected internal override bool GetNextNetworkObjectId(out int nextNetworkObjectId)
        {
            if (!_objectIdCache.TryDequeue(out nextNetworkObjectId))
            {
                nextNetworkObjectId = NetworkObject.UNSET_OBJECTID_VALUE;
                NetworkManager.LogError($"No more available ObjectIds. How the heck did you manage to have {ushort.MaxValue} objects spawned at once?");
            }

            return nextNetworkObjectId != NetworkObject.UNSET_OBJECTID_VALUE;
        }
        #endregion

        #region Initializing Objects In Scenes.
        /// <summary>
        /// Called when a scene load starts.
        /// </summary>
        private void SceneManager_OnLoadStart(Scened.SceneLoadStartEventArgs obj)
        {
            _scenesLoading = true;
        }

        /// <summary>
        /// Called after the active scene has been scene, immediately after scene loads.
        /// </summary>
        private void SceneManager_OnActiveSceneSet(bool asServer)
        {
            _scenesLoading = false;
            IterateLoadedScenes(true);
        }

        /// <summary>
        /// Iterates loaded scenes and sets them up.
        /// </summary>
        /// <param name = "ignoreFrameRestriction">True to ignore the frame restriction when iterating.</param>
        internal void IterateLoadedScenes(bool ignoreFrameRestriction)
        {
            // Not started, clear loaded scenes.
            if (!NetworkManager.ServerManager.Started)
            {
                ClearSceneLoadedNetworkObjects();
                return;
            }

            for (int i = 0; i < _loadedSceneNetworkObjects.Count; i++)
            {
                (int frame, List<NetworkObject> networkObjects) value = _loadedSceneNetworkObjects[i];
                if (ignoreFrameRestriction || Time.frameCount > value.frame)
                {
                    SetupSceneObjects(value.networkObjects);
                    CollectionCaches<NetworkObject>.Store(value.networkObjects);
                    _loadedSceneNetworkObjects.RemoveAt(i);
                    i--;
                }
            }
        }

        /// <summary>
        /// Called when a scene loads on the server.
        /// </summary>
        /// <param name = "s"></param>
        /// <param name = "arg1"></param>
        protected internal override void SceneManager_sceneLoaded(Scene s, LoadSceneMode arg1)
        {
            base.SceneManager_sceneLoaded(s, arg1);

            if (!NetworkManager.ServerManager.Started)
                return;

            List<NetworkObject> sceneNobs = CollectionCaches<NetworkObject>.RetrieveList();
            Scenes.GetSceneNetworkObjects(s, firstOnly: false, errorOnDuplicates: true, ignoreUnsetSceneIds: true, ref sceneNobs);
            _loadedSceneNetworkObjects.Add((Time.frameCount, sceneNobs));

            InitializeRootNetworkObjects(sceneNobs);
        }

        /// <summary>
        /// Sets initial values for NetworkObjects.
        /// </summary>
        /// <param name = "nobs"></param>
        private void InitializeRootNetworkObjects(List<NetworkObject> nobs)
        {
            /* First update the nested status on all nobs, as well
             * set them as not initialized. This is done as some scene objets might be prefabs
             * that were changed in scene but have not had the prefab settings updated to those
             * changes. */
            foreach (NetworkObject nob in nobs)
            {
                nob.SetIsNestedThroughTraversal();
                nob.UnsetHasBeenInitialized();
            }

            // Initialize sceneNobs cache, but do not invoke callbacks till next frame.
            foreach (NetworkObject nob in nobs)
            {
                if (nob.IsSceneObject && !nob.IsNested)
                    nob.SetInitializedValues(parentNob: null, ignoreSerializedTimestamp: false);
            }
        }

        /// <summary>
        /// Setup all NetworkObjects in scenes. Should only be called when server is active.
        /// </summary>
        protected internal void SetupSceneObjects()
        {
            Scene ddolScene = DDOL.GetDDOL().gameObject.scene;
            bool ddolLoaded = ddolScene.isLoaded;

            /* Becomes false if setup in GetSceenAt.
             * This is a safety check for if Unity ever changes
             * the behavior where DDOL scenes appear in the sceneCount. */
            bool trySetupDdol = true;

            for (int i = 0; i < SceneManager.sceneCount; i++)
            {
                Scene s = SceneManager.GetSceneAt(i);
                if (ddolLoaded && s.handle == ddolScene.handle)
                    trySetupDdol = false;

                SetupSceneObjects(s);
            }

            if (trySetupDdol)
                SetupSceneObjects(ddolScene);
        }

        /// <summary>
        /// Setup NetworkObjects in a scene. Should only be called when server is active.
        /// </summary>
        /// <param name = "s"></param>
        private void SetupSceneObjects(Scene s)
        {
            if (!s.IsValid())
                return;

            List<NetworkObject> sceneNobs = CollectionCaches<NetworkObject>.RetrieveList();
            Scenes.GetSceneNetworkObjects(s, firstOnly: false, errorOnDuplicates: true, ignoreUnsetSceneIds: true, ref sceneNobs);

            SetupSceneObjects(sceneNobs);

            CollectionCaches<NetworkObject>.Store(sceneNobs);
        }

        /// <summary>
        /// Setup NetworkObjects in a scene. Should only be called when server is active.
        /// </summary>
        /// <param name = "s"></param>
        private void SetupSceneObjects(List<NetworkObject> sceneNobs)
        {
            /* Run a sanity check to make sure the user did not
             * delete any NetworkObjects between the scene loading and now. */
            for (int i = 0; i < sceneNobs.Count; i++)
            {
                if (sceneNobs[i] == null)
                    sceneNobs.RemoveAt(i--);
            }

            InitializeRootNetworkObjects(sceneNobs);

            List<NetworkObject> cache = SortRootAndNestedByInitializeOrder(sceneNobs);

            bool isHost = NetworkManager.IsHostStarted;
            int nobsCount = cache.Count;
            for (int i = 0; i < nobsCount; i++)
            {
                NetworkObject nob = cache[i];

                // Requirement: parent spawned or no parent, isNetworked, isSceneObject, and is deinitialized (not already spawned/spawning).
                bool parentIsSpawned = nob.CurrentParentNetworkBehaviour == null || nob.CurrentParentNetworkBehaviour.IsSpawned;
                if (parentIsSpawned && nob.GetIsNetworked() && nob.IsSceneObject && nob.IsDeinitializing)
                {
                    if (!nob.WasActiveDuringEdit_Set1)
                    {
                        NetworkManager.LogError($"NetworkObject {nob.name} in scene {nob.gameObject.scene.name} needs to be reserialized. Please use the Fish-Networking menu -> Utility -> Reserialize NetworkObjects.");
                        continue;
                    }

                    AddToSceneObjects(nob);
                    /* If was active in the editor (before hitting play), or currently active
                     * then PreInitialize without synchronizing to clients. There is no reason
                     * to synchronize to clients because the scene just loaded on server,
                     * which means clients are not yet in the scene. */
                    if (nob.WasActiveDuringEdit || nob.gameObject.activeInHierarchy)
                    {
                        // If not host then object doesn't need to be spawned until a client joins.
                        if (!isHost)
                            SetupWithoutSynchronization(nob);
                        // Otherwise spawn object so observers update for clientHost.
                        else
                            SpawnWithoutChecks(nob);
                    }
                }
            }

            CollectionCaches<NetworkObject>.Store(cache);
        }

        /// <summary>
        /// Performs setup on a NetworkObject without synchronizing the actions to clients.
        /// </summary>
        /// <param name = "objectId">Override ObjectId to use.</param>
        private bool SetupWithoutSynchronization(NetworkObject nob, NetworkConnection ownerConnection = null, int? objectId = null, bool initializeEarly = true)
        {
            if (nob.GetIsNetworked())
            {
                int objectIdValue;

                if (objectId != null)
                {
                    objectIdValue = objectId.Value;
                }
                else
                {
                    if (!GetNextNetworkObjectId(out objectIdValue))
                        return false;
                }

                if (initializeEarly)
                    nob.InitializeEarly(NetworkManager, objectIdValue, ownerConnection, true);

                base.AddToSpawned(nob, true);
                nob.gameObject.SetActive(true);
                nob.Initialize(true, true);

                return true;
            }

            return false;
        }
        #endregion

        #region Spawning.
        /// <summary>
        /// Spawns an object over the network.
        /// </summary>
        internal void Spawn(NetworkObject networkObject, NetworkConnection ownerConnection = null, Scene scene = default)
        {
            // Default as false, will change if needed.
            bool predictedSpawn = false;

            if (networkObject == null)
            {
                NetworkManager.LogError($"Specified networkObject is null.");
                return;
            }

            //Unset the sceneId on the instantiated copy.
            networkObject.SceneId = 0;
            
            if (!NetworkManager.ServerManager.Started)
            {
                // Neither server nor client are started.
                if (!NetworkManager.ClientManager.Started)
                {
                    NetworkManager.LogWarning("Cannot spawn object because server nor client are active.");
                    return;
                }

                // Server has predicted spawning disabled.
                if (!NetworkManager.ServerManager.GetAllowPredictedSpawning())
                {
                    NetworkManager.LogWarning("Cannot spawn object because server is not active and predicted spawning is not enabled.");
                    return;
                }

                // Various predicted spawn checks.
                if (!CanPredictedSpawn(networkObject, NetworkManager.ClientManager.Connection, false))
                    return;

                // Since server is not started then run TrySpawn for client, given this is a client trying to predicted spawn.
                if (!networkObject.PredictedSpawn.OnTrySpawnClient())
                    return;

                predictedSpawn = true;
            }

            if (!networkObject.gameObject.scene.IsValid())
            {
                NetworkManager.LogError($"{networkObject.name} is a prefab. You must instantiate the prefab first, then use Spawn on the instantiated copy.");
                return;
            }

            if (ownerConnection != null && ownerConnection.IsActive && !ownerConnection.LoadedStartScenes(!predictedSpawn))
            {
                NetworkManager.LogWarning($"{networkObject.name} was spawned but it's recommended to not spawn objects for connections until they have loaded start scenes. You can be notified when a connection loads start scenes by using connection.OnLoadedStartScenes on the connection, or SceneManager.OnClientLoadStartScenes.");
            }

            if (networkObject.IsSpawned)
            {
                NetworkManager.LogWarning($"{networkObject.name} is already spawned.");
                return;
            }

            NetworkBehaviour networkBehaviourParent = networkObject.CurrentParentNetworkBehaviour;
            if (networkBehaviourParent != null && !networkBehaviourParent.IsSpawned)
            {
                NetworkManager.LogError($"{networkObject.name} cannot be spawned because it has a parent NetworkObject {networkBehaviourParent} which is not spawned.");
                return;
            }

            /* If scene is specified make sure the object is root,
             * and if not move it before network spawning. */
            if (scene.IsValid())
            {
                if (networkObject.transform.parent != null)
                {
                    NetworkManager.LogError($"{networkObject.name} cannot be moved to scene name {scene.name}, handle {scene.handle} because {networkObject.name} is not root and only root objects may be moved.");
                    return;
                }
                else
                {
                    SceneManager.MoveGameObjectToScene(networkObject.gameObject, scene);
                }
            }

            if (predictedSpawn)
                NetworkManager.ClientManager.Objects.PredictedSpawn(networkObject, ownerConnection);
            else
                SpawnWithoutChecks(networkObject, recursiveSpawnCache: null, ownerConnection);
        }

        /// <summary>
        /// Spawns networkObject without any checks.
        /// </summary>
        private void SpawnWithoutChecks(NetworkObject networkObject, List<NetworkObject> recursiveSpawnCache = null, NetworkConnection ownerConnection = null, int? objectId = null, bool rebuildObservers = true, bool initializeEarly = true, bool isRecursiveIteration = false)
        {
            /* Setup locally without sending to clients.
             * When observers are built for the network object
             * during initialization spawn messages will
             * be sent. */
            networkObject.SetIsNetworked(true);

            /* Grab the nested before spawning the networkObject. This prevents double initialization
             * if one of the OnStart callbacks adds nested to networkObject.
             *
             * EG: called on NetworkObject during 'SetupWithoutSynchronization'
             * private void OnStartNetwork()
             * {
             *       NetworkObject n = Instantiate(xyz);
             *       n.SetParent(this); // this being NetworkObject
             *       base.Spawn(n);
             * }
             *
             * If nested was fetched after SetupWithout Synchronize just below then it would
             * include the newly nested object, and try to initialize it twice.
             */
            List<NetworkObject> nestedNetworkObjects = isRecursiveIteration ? null : networkObject.GetNetworkObjects(GetNetworkObjectOption.AllNestedRecursive);

            if (SetupWithoutSynchronization(networkObject, ownerConnection, objectId, initializeEarly))
                _spawnCache.Add(networkObject);

            if (nestedNetworkObjects != null)
            {
                foreach (NetworkObject item in nestedNetworkObjects)
                {
                    /* Only spawn recursively if the nob state is unset.
                     * Unset indicates that the nob has not been manually spawned or despawned. */
                    if (item.gameObject.activeInHierarchy || item.State == NetworkObjectState.Spawned)
                        SpawnWithoutChecks(item, recursiveSpawnCache: null, ownerConnection, isRecursiveIteration: true);
                }
            }

            /* Copy to a new cache then reset _spawnCache
             * just incase rebuilding observers would lead to
             * more additions into _spawnCache. EG: rebuilding
             * may result in additional objects being spawned
             * for clients and if _spawnCache were not reset
             * the same objects would be rebuilt again. This likely
             * would not affect anything other than perf but who
             * wants that. */
            bool recursiveCacheWasNull = recursiveSpawnCache == null;
            if (recursiveCacheWasNull)
                recursiveSpawnCache = CollectionCaches<NetworkObject>.RetrieveList();
            recursiveSpawnCache.AddRange(_spawnCache);
            _spawnCache.Clear();

            // Also rebuild observers for the object so it spawns for others.
            if (rebuildObservers)
                RebuildObservers(recursiveSpawnCache);

            int spawnCacheCopyCount = recursiveSpawnCache.Count;
            /* If also client then we need to make sure the object renderers have correct visibility.
             * Set visibility based on if the observers contains the clientHost connection. */
            if (NetworkManager.IsClientStarted)
            {
                int count = spawnCacheCopyCount;
                NetworkConnection localConnection = NetworkManager.ClientManager.Connection;
                for (int i = 0; i < count; i++)
                {
                    NetworkObject nob = recursiveSpawnCache[i];
                    nob.SetRenderersVisible(nob.Observers.Contains(localConnection));
                }
            }

            CollectionCaches<NetworkObject>.StoreAndDefault(ref nestedNetworkObjects);

            /* If collection was null then store the one retrieved.
             * Otherwise, let the calling method handle the provided
             * cache. */
            if (recursiveCacheWasNull)
                CollectionCaches<NetworkObject>.Store(recursiveSpawnCache);
        }

        /// <summary>
        /// Reads a predicted spawn.
        /// </summary>
        internal void ReadSpawn(PooledReader reader, NetworkConnection conn)
        {
            ushort spawnLength = reader.ReadUInt16Unpacked();

            int readStartPosition = reader.Position;

            SpawnType st = (SpawnType)reader.ReadUInt8Unpacked();
            bool sceneObject = st.FastContains(SpawnType.Scene);
            bool isGlobal = st.FastContains(SpawnType.InstantiatedGlobal);

            ReadNestedSpawnIds(reader, st, out byte? nobComponentId, out int? parentObjectId, out byte? parentComponentId, readSpawningObjects: null);

            int objectId = reader.ReadNetworkObjectForSpawn(out _, out ushort collectionId);

            // No predicted spawn ids left. Should not be possible as client would have stopped this locally.
            if (conn.PredictedObjectIds.Count == 0 || !conn.PredictedObjectIds.TryDequeue(out int serverPredictedObjectId))
            {
                reader.Clear();
                conn.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {conn.ClientId} used predicting spawning without any Ids in queue.");
                return;
            }

            // Ids should match. Client cannot send predicted spawn Ids out of order, so they should always match the server in order.
            if (objectId != serverPredictedObjectId)
            {
                reader.Clear();
                conn.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {conn.ClientId} used predicted Id of {objectId} while the server Id is {serverPredictedObjectId}.");
                return;
            }

            NetworkObject nob = null;
            NetworkConnection owner = null;

            /* See if the parent exists. If not, then do not
             * continue and send failed response to client. */
            if (parentObjectId.HasValue && !Spawned.TryGetValueIL2CPP(parentObjectId.Value, out NetworkObject _))
            {
                NetworkManager.Log($"Predicted spawn failed due to the NetworkObject's parent not being found. Scene object: {sceneObject}, ObjectId {objectId}, CollectionId {collectionId}.");
                SendFailedResponse(objectId);
                return;
            }
            owner = reader.ReadNetworkConnection();

            // Read transform values which differ from serialized values.
            ReadTransformProperties(reader, out Vector3? nullablePosition, out Quaternion? nullableRotation, out Vector3? nullableScale);

            int prefabId;
            ulong sceneId = 0;
            string sceneName = string.Empty;
            string objectName = string.Empty;

            if (sceneObject)
            {
                ReadSceneObjectId(reader, out sceneId);
#if DEVELOPMENT
                if (NetworkManager.ClientManager.IsServerDevelopment)
                    CheckReadSceneObjectDetails(reader, ref sceneName, ref objectName);
#endif
                nob = GetSceneNetworkObject(sceneId, sceneName, objectName);
            }
            else
            {
                prefabId = reader.ReadNetworkObjectId();
                ObjectPoolRetrieveOption retrieveOptions = ObjectPoolRetrieveOption.MakeActive | ObjectPoolRetrieveOption.LocalSpace;
                nob = NetworkManager.GetPooledInstantiated(prefabId, collectionId, retrieveOptions, null, nullablePosition, nullableRotation, nullableScale, false);
            }

            /* NetworkObject could not be found. User could be sending an invalid Id,
             * or perhaps it was a scene object and the scene had unloaded prior to getting
             * this spawn message. */
            if (nob == null)
            {
                NetworkManager.Log($"Predicted spawn failed due to the NetworkObject not being found. Scene object: {sceneObject}, ObjectId {objectId}, CollectionId {collectionId}.");
                SendFailedResponse(objectId);
                return;
            }

            if (!nob.WasActiveDuringEdit_Set1)
            {
                string sceneOrPrefabText = sceneObject ? $"in scene {nob.gameObject.scene.name}" : "prefab";
                NetworkManager.LogError($"NetworkObject {nob.name} {sceneOrPrefabText}. Please use the Fish-Networking menu -> Utility -> Reserialize NetworkObjects.");
            }


            /* Update sceneObject position.
             * There is no need to do this on instantiate since the position is set
             * during the instantiation. */
            if (sceneObject)
            {
                nob.transform.SetLocalPositionRotationAndScale(nullablePosition, nullableRotation, nullableScale);
            }

            // Check if nob allows predicted spawning.
            if (!CanPredictedSpawn(nob, conn, true, reader))
                return;

            nob.SetIsGlobal(isGlobal);
            nob.SetIsNetworked(true);
            nob.InitializeEarly(NetworkManager, objectId, owner, true);
            // Initialize for prediction.
            nob.InitializePredictedObject_Server(conn);

            ReadPayload(conn, nob, reader);
            ReadRpcLinks(reader);
            ReadSyncTypesForSpawn(reader);

            // Check user implementation of trySpawn.
            if (!nob.PredictedSpawn.OnTrySpawnServer(conn, owner))
            {
                // Inform client of failure.
                SendFailedResponse(objectId);
                return;
            }

            // Once here everything is good.

            // Get connections to send spawn to.
            List<NetworkConnection> conns = RetrieveAuthenticatedConnections();

            SendSuccessResponse(objectId);
            // Store caches used.
            CollectionCaches<NetworkConnection>.Store(conns);

            int trafficWritten;

            // Sends a failed response.
            void SendFailedResponse(int lObjectId)
            {
                SkipRemainingSpawnLength();
                if (nob != null)
                {
                    // TODO support pooling. This first requires a rework of the initialization / clientHost message system.
                    UnityEngine.Object.Destroy(nob.gameObject);
                    // base.NetworkManager.StorePooledInstantiated(nob, true);
                }

                PooledWriter writer = WriteResponseHeader(success: false, lObjectId);

                trafficWritten = writer.Length;

                conn.SendToClient((byte)Channel.Reliable, writer.GetArraySegment());
                WriterPool.Store(writer);
            }

            // Sends a success spawn result and returns nobs recursively spawned, including original.
            void SendSuccessResponse(int lObjectId)
            {
                PooledWriter writer = WriteResponseHeader(success: true, lObjectId);

                trafficWritten = writer.Length;

                SpawnWithoutChecks(nob, recursiveSpawnCache: null, owner, lObjectId, rebuildObservers: true, initializeEarly: false);
                conn.SendToClient((byte)Channel.Reliable, writer.GetArraySegment());

                WriterPool.Store(writer);
            }

            // Writes response header and returns writer used.
            PooledWriter WriteResponseHeader(bool success, int lObjectId)
            {
                PooledWriter writer = WriterPool.Retrieve();
                writer.WritePacketIdUnpacked(PacketId.PredictedSpawnResult);
                writer.WriteBoolean(success);

                // Id of object which was predicted spawned.
                writer.WriteNetworkObjectId(lObjectId);

                // Write the next Id even if not succesful.
                int nextId;
                if (NetworkManager.ServerManager.Objects.GetObjectIdCache().TryDequeue(out nextId))
                    conn.PredictedObjectIds.Enqueue(nextId);
                else
                    nextId = NetworkObject.UNSET_OBJECTID_VALUE;

                // Write nextId even if invalid. Client will not add invalid Ids.
                writer.WriteNetworkObjectId(nextId);

                return writer;
            }

#if DEVELOPMENT && !UNITY_SERVER
            if (NetworkTrafficStatistics != null)
                NetworkTrafficStatistics.AddOutboundPacketIdData(PacketId.PredictedSpawnResult, string.Empty, trafficWritten, gameObject: null, asServer: true);
#endif

            // Skips remaining data for the spawn.
            void SkipRemainingSpawnLength()
            {
                /* Simply setting the position to readStart + spawnLength works
                 * too but when possible use supplied API. */
                int skipLength = spawnLength - (reader.Position - readStartPosition);
                reader.Skip(skipLength);
            }
        }
        #endregion

        #region Despawning.
        /// <summary>
        /// Cleans recently despawned objects.
        /// </summary>
        private void CleanRecentlyDespawned()
        {
            // Only iterate if frame ticked to save perf.
            if (!NetworkManager.TimeManager.FrameTicked)
                return;

            List<int> intCache = CollectionCaches<int>.RetrieveList();

            uint requiredTicks = _cleanRecentlyDespawnedMaxTicks;
            uint currentTick = NetworkManager.TimeManager.LocalTick;
            // Iterate 20, or 5% of the collection, whichever is higher.
            int iterations = Mathf.Max(20, (int)(RecentlyDespawnedIds.Count * 0.05f));
            /* Given this is a dictionary there is no gaurantee which order objects are
             * added. Because of this it's possible some objects may take much longer to
             * be removed. This is okay so long as a consistent chunk of objects are removed
             * at a time; eventually all objects will be iterated. */
            int count = 0;
            foreach (KeyValuePair<int, uint> kvp in RecentlyDespawnedIds)
            {
                long result = currentTick - kvp.Value;
                // If enough ticks have passed to remove.
                if (result > requiredTicks)
                    intCache.Add(kvp.Key);

                count++;
                if (count == iterations)
                    break;
            }

            // Remove cached entries.
            int cCount = intCache.Count;
            for (int i = 0; i < cCount; i++)
                RecentlyDespawnedIds.Remove(intCache[i]);

            CollectionCaches<int>.Store(intCache);
        }

        /// <summary>
        /// Returns if an objectId was recently despawned.
        /// </summary>
        /// <param name = "objectId">ObjectId to check.</param>
        /// <param name = "ticks">Passed ticks to be within to be considered recently despawned.</param>
        /// <returns>True if an objectId was despawned with specified number of ticks.</returns>
        public bool RecentlyDespawned(int objectId, uint ticks)
        {
            uint despawnTick;
            if (!RecentlyDespawnedIds.TryGetValue(objectId, out despawnTick))
                return false;

            return NetworkManager.TimeManager.LocalTick - despawnTick <= ticks;
        }

        /// <summary>
        /// Adds to objects pending destroy due to clientHost environment.
        /// </summary>
        /// <param name = "nob"></param>
        internal void AddToPending(NetworkObject nob)
        {
            _pendingDestroy.Add(nob);
        }

        /// <summary>
        /// Tries to removes objectId from PendingDestroy and returns if successful.
        /// </summary>
        internal bool RemoveFromPending(NetworkObject nob)
        {
            return _pendingDestroy.Remove(nob);
        }

        /// <summary>
        /// Returns a NetworkObject in PendingDestroy.
        /// </summary>
        internal NetworkObject GetFromPending(int objectId)
        {
            /* Becomes true if there was a null entry and pending must be rebuilt.
             * This would be very uncommon */
            bool rebuildPending = false;

            foreach (NetworkObject n in _pendingDestroy)
            {
                if (n == null)
                {
                    rebuildPending = true;
                    continue;
                }

                if (n.ObjectId == objectId)
                    return n;
            }

            if (rebuildPending)
            {
                HashSet<NetworkObject> newPending = CollectionCaches<NetworkObject>.RetrieveHashSet();
                foreach (NetworkObject n in _pendingDestroy)
                {
                    if (n != null)
                        newPending.Add(n);
                }

                CollectionCaches<NetworkObject>.Store(_pendingDestroy);
                _pendingDestroy = newPending;
            }

            //Fall through, nothing found.
            return null;
        }

        /// <summary>
        /// Destroys NetworkObjects pending for destruction.
        /// </summary>
        internal void DestroyPending()
        {
            foreach (NetworkObject item in _pendingDestroy)
            {
                if (item != null)
                    UnityEngine.Object.Destroy(item.gameObject);
            }

            _pendingDestroy.Clear();
        }

        /// <summary>
        /// Despawns an object over the network.
        /// </summary>
        internal override void Despawn(NetworkObject networkObject, DespawnType despawnType, bool asServer)
        {
            //Default as false, will change if needed.
            bool predictedDespawn = false;

            if (networkObject == null)
            {
                NetworkManager.LogWarning($"NetworkObject cannot be despawned because it is null.");
                return;
            }

            if (networkObject.IsDeinitializing)
            {
                NetworkManager.LogWarning($"Object {networkObject.name} cannot be despawned because it is already deinitializing.");
                return;
            }

            if (!NetworkManager.ServerManager.Started)
            {
                //Neither server nor client are started.
                if (!NetworkManager.ClientManager.Started)
                {
                    NetworkManager.LogWarning("Cannot despawn object because server nor client are active.");
                    return;
                }

                //Server has predicted spawning disabled.
                if (!NetworkManager.ServerManager.GetAllowPredictedSpawning())
                {
                    NetworkManager.LogWarning("Cannot despawn object because server is not active and predicted spawning is not enabled.");
                    return;
                }

                //Various predicted despawn checks.
                if (!CanPredictedDespawn(networkObject, NetworkManager.ClientManager.Connection, false))
                    return;

                predictedDespawn = true;
            }

            if (!networkObject.gameObject.scene.IsValid())
            {
                NetworkManager.LogError($"{networkObject.name} is a prefab. You must instantiate the prefab first, then use Spawn on the instantiated copy.");
                return;
            }

            if (predictedDespawn)
            {
                NetworkManager.ClientManager.Objects.PredictedDespawn(networkObject);
            }
            else
            {
                FinalizeDespawn(networkObject, despawnType);
                RecentlyDespawnedIds[networkObject.ObjectId] = NetworkManager.TimeManager.LocalTick;
                base.Despawn(networkObject, despawnType, asServer);
            }
        }

        /// <summary>
        /// Called when a NetworkObject is destroyed without being deactivated first.
        /// </summary>
        /// <param name = "nob"></param>
        internal override void NetworkObjectDestroyed(NetworkObject nob, bool asServer)
        {
            //Only finalize despawn if not already deinitialized.
            if (!nob.IsDeinitializing)
                FinalizeDespawn(nob, DespawnType.Destroy);

            base.NetworkObjectDestroyed(nob, asServer);
        }

        /// <summary>
        /// Finalizes the despawn process. By the time this is called the object is considered unaccessible.
        /// </summary>
        /// <param name = "nob"></param>
        private void FinalizeDespawn(NetworkObject nob, DespawnType despawnType)
        {
            List<NetworkBehaviour> dirtiedNbs = _dirtySyncTypeBehaviours;
            if (nob != null && nob.ObjectId != NetworkObject.UNSET_OBJECTID_VALUE)
            {
                // Write out any pending sync types and be sure to clear from the dirty list
                // to avoid trying to write out a despawned object later on.
                for (int i = 0, count = nob.NetworkBehaviours.Count; i < count; ++i)
                {
                    NetworkBehaviour nb = nob.NetworkBehaviours[i];
                    if (nb.SyncTypeDirty && nb.WriteDirtySyncTypes(SyncTypeWriteFlag.ForceReliable | SyncTypeWriteFlag.IgnoreInterval))
                        dirtiedNbs.Remove(nb);
                }

                WriteDespawnAndSend(nob, despawnType);

                CacheObjectId(nob);
            }
        }

        /// <summary>
        /// Writes a despawn and sends it to clients.
        /// </summary>
        private void WriteDespawnAndSend(NetworkObject nob, DespawnType despawnType)
        {
            HashSet<NetworkConnection> observers = nob.Observers;
            if (observers.Count == 0)
                return;

            PooledWriter everyoneWriter = WriterPool.Retrieve();
            WriteDespawn(nob, despawnType, everyoneWriter);

            ArraySegment<byte> despawnSegment = everyoneWriter.GetArraySegment();

            //Add observers to a list cache.
            List<NetworkConnection> cache = CollectionCaches<NetworkConnection>.RetrieveList();
            /* Must be added into a new collection because the
             * user might try and modify the observers in the despawn, which
             * would cause a collection modified error. */
            cache.AddRange(observers);
            int cacheCount = cache.Count;

            for (int i = 0; i < cacheCount; i++)
            {
                //Invoke ondespawn and send despawn.
                NetworkConnection conn = cache[i];
                nob.InvokeOnServerDespawn(conn);
                NetworkManager.TransportManager.SendToClient((byte)Channel.Reliable, despawnSegment, conn);
                //Remove from observers.
                //nob.Observers.Remove(conn);
            }

            everyoneWriter.Store();
            CollectionCaches<NetworkConnection>.Store(cache);
        }

        /// <summary>
        /// Reads a predicted despawn.
        /// </summary>
        internal void ReadDespawn(Reader reader, NetworkConnection conn)
        {
            NetworkObject nob = reader.ReadNetworkObject();

            //Maybe server destroyed the object so don't kick if null.
            if (nob == null)
                return;
            if (nob.IsDeinitializing)
                return;

            //Various predicted despawn checks.
            if (!CanPredictedDespawn(nob, conn, true))
                return;

            //Despawn object.
            nob.Despawn();
        }
        #endregion
    }
}