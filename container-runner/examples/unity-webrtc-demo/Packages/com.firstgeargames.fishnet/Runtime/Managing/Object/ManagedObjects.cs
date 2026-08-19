#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Component.Observing;
using FishNet.Connection;
using FishNet.Managing.Logging;
using FishNet.Managing.Utility;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using FishNet.Managing.Server;
using FishNet.Managing.Statistic;
using UnityEngine;
using UnityEngine.SceneManagement;
using Unity.Profiling;

namespace FishNet.Managing.Object
{
    public abstract partial class ManagedObjects
    {
        #region Public.
        /// <summary>
        /// NetworkObjects which are currently active.
        /// </summary>
        public IReadOnlyDictionary<int, NetworkObject> Spawned => _spawned;
        private Dictionary<int, NetworkObject> _spawned = new();
        /// <summary>
        /// Invoked when an object is added to Spawned.
        /// </summary>
        public event OnSpawnedChanged OnSpawnedAdd;
        /// <summary>
        /// Invoked when an object is removed from Spawned.
        /// </summary>
        public event OnSpawnedChanged OnSpawnedRemove;
        /// <summary>
        /// Invoked when Spawned is cleared.
        /// </summary>
        public event Action OnSpawnedClear;

        /// <summary>
        /// Delegate for when there is change to Spawned.
        /// </summary>
        public delegate void OnSpawnedChanged(int objectId, NetworkObject networkObject);
        #endregion

        #region Private Profiler Markers
        private static readonly ProfilerMarker _pm_ParseReplicateRpc = new("ManagedObjects.ParseReplicateRpc(PooledReader, NetworkConnection, Channel)");
        #endregion

        #region Protected.
        /// <summary>
        /// Returns the next ObjectId to use.
        /// </summary>
        protected internal virtual bool GetNextNetworkObjectId(out int nextNetworkObjectId)
        {
            nextNetworkObjectId = NetworkObject.UNSET_OBJECTID_VALUE;
            return false;
        }

        /// <summary>
        /// NetworkManager handling this.
        /// </summary>
        protected NetworkManager NetworkManager { get; private set; }
        /// <summary>
        /// Objects in currently loaded scenes. These objects can be active or inactive.
        /// Key is the objectId while value is the object. Key is not the same as NetworkObject.ObjectId.
        /// </summary>
        protected Dictionary<ulong, NetworkObject> SceneObjects_Internal = new();
        /// <summary>
        /// Objects in currently loaded scenes. These objects can be active or inactive.
        /// Key is the objectId while value is the object. Key is not the same as NetworkObject.ObjectId.
        /// </summary>
        public IReadOnlyDictionary<ulong, NetworkObject> SceneObjects => SceneObjects_Internal;
        /// <summary>
        /// </summary>
        [NonSerialized]
        protected NetworkTrafficStatistics NetworkTrafficStatistics;

        /// <summary>
        /// Called to add an object to Spawned.
        /// </summary>
        protected void AddtoSpawnedCollectionAndInvoke(NetworkObject nob)
        {
            _spawned[nob.ObjectId] = nob;
            OnSpawnedAdd?.Invoke(nob.ObjectId, nob);
        }

        /// <summary>
        /// Called to remove an object from Spawned.
        /// </summary>
        protected void RemoveFromSpawnedCollectionAndInvoke(NetworkObject nob)
        {
            if (_spawned.Remove(nob.ObjectId))
                OnSpawnedRemove?.Invoke(nob.ObjectId, nob);
        }

        /// <summary>
        /// Called to clear Spawned.
        /// </summary>
        protected void ClearSpawnedCollectionAndInvoke()
        {
            _spawned.Clear();
            OnSpawnedClear?.Invoke();
        }
        #endregion

        #region Private.
        /// <summary>
        /// Cached HashGrid. Will be null if not used.
        /// </summary>
        private HashGrid _hashGrid;
        #endregion

        protected virtual void Initialize(NetworkManager manager)
        {
            NetworkManager = manager;
            manager.StatisticsManager.TryGetNetworkTrafficStatistics(out NetworkTrafficStatistics);

            manager.TryGetInstance(out _hashGrid);
        }

        /// <summary>
        /// Subscribes to SceneManager.SceneLoaded event.
        /// </summary>
        /// <param name = "subscribe"></param>
        internal void SubscribeToSceneLoaded(bool subscribe)
        {
            if (subscribe)
                SceneManager.sceneLoaded += SceneManager_sceneLoaded;
            else
                SceneManager.sceneLoaded -= SceneManager_sceneLoaded;
        }

        /// <summary>
        /// Called when a scene is loaded.
        /// </summary>
        /// <param name = "s"></param>
        /// <param name = "arg1"></param>
        protected internal virtual void SceneManager_sceneLoaded(Scene s, LoadSceneMode arg1) { }

        /// <summary>
        /// Called when a NetworkObject runs Deactivate.
        /// </summary>
        /// <param name = "nob"></param>
        internal virtual void NetworkObjectDestroyed(NetworkObject nob, bool asServer)
        {
            if (nob == null)
                return;

            RemoveFromSpawned(nob, fromOnDestroy: true, asServer);
        }

        /// <summary>
        /// Attempts to remove an old ObjectId when an object had called Initialize without deinitializing first.
        /// </summary>
        /// <returns>True if clean-up was successful.</returns>
        internal void ObjectInitializedWithoutDeinitializing(int oldId, NetworkObject callingNetworkObject)
        {
            // Check for the oldId in spawned.
            if (_spawned.TryGetValueIL2CPP(oldId, out NetworkObject oldNetworkObject))
            {
                /* If the old networkObject is the same as new then remove
                 * the old entry from spawned and return the Id. */
                if (callingNetworkObject == oldNetworkObject || oldNetworkObject == null)
                {
                    _spawned.Remove(oldId);
                    if (this is ServerObjects serverObjects)
                        serverObjects.CacheObjectId(oldId);

                    return;
                }
                
                NetworkManager.LogError($"Initialization occurred twice on object {this.ToString()} and recovery not clean up as expected. The prior Id of [{oldId}] belonged to a different object {oldNetworkObject} when it was expected to belong to the first.");
            }
        }

        /// <summary>
        /// Removes a NetworkedObject from spawned.
        /// </summary>
        protected virtual void RemoveFromSpawned(NetworkObject nob, bool fromOnDestroy, bool asServer)
        {
            RemoveFromSpawnedCollectionAndInvoke(nob);

            // Do the same with SceneObjects.
            if (fromOnDestroy && nob.IsSceneObject)
                RemoveFromSceneObjects(nob);
        }

        /// <summary>
        /// Despawns a NetworkObject.
        /// </summary>
        internal virtual void Despawn(NetworkObject nob, DespawnType despawnType, bool asServer)
        {
            if (nob == null)
            {
                NetworkManager.LogWarning($"Cannot despawn a null NetworkObject.");
                return;
            }

            /* If not asServer and the object is not initialized on client
             * then it likely is already despawned. This bit of code should
             * never be reached as checks should be placed before-hand. */
            if (!asServer && !nob.IsClientInitialized)
            {
                NetworkManager.LogError($"Object {nob.ToString()} is already despawned. Please report this error.");
                return;
            }


            // True if should be destroyed, false if deactivated.
            bool destroy = false;
            bool wasRemovedFromPending = false;

            /* Only modify object state if asServer,
             * or !asServer and not host. This is so clients, when acting as
             * host, don't destroy objects they lost observation of. */

            /* Nested prefabs can never be destroyed. Only check to
             * destroy if not nested. By nested prefab, this means the object
             * despawning is part of another prefab that is also a spawned
             * network object. */
            if (!nob.IsNested)
            {
                // If as server.
                if (asServer)
                {
                    // Scene object.
                    if (!nob.IsSceneObject)
                    {
                        /* If client-host has visibility
                         * then disable and wait for client-host to get destroy
                         * message. Otherwise destroy immediately. */
                        if (nob.Observers.Contains(NetworkManager.ClientManager.Connection))
                            NetworkManager.ServerManager.Objects.AddToPending(nob);
                        else
                            destroy = true;
                    }
                }
                // Not as server.
                else
                {
                    bool isServer = NetworkManager.IsServerStarted;
                    // Only check to destroy if not a scene object.
                    if (!nob.IsSceneObject)
                    {
                        /* If was removed from pending then also destroy.
                         * Pending objects are ones that exist on the server
                         * side only to await destruction from client side.
                         * Objects can also be destroyed if server is not
                         * active. */
                        wasRemovedFromPending = NetworkManager.ServerManager.Objects.RemoveFromPending(nob);
                        destroy = !isServer || wasRemovedFromPending;
                    }
                }
            }

            TryUnsetParent();

            /* If this had a parent set at runtime then
             * unset parent before checks are completed.
             * If we did not do this then this nob would
             * just be disabled beneath its runtime parent,
             * when it should be pooled separately or destroyed. */
            void TryUnsetParent()
            {
                if (!asServer || wasRemovedFromPending)
                {
                    if (nob.RuntimeParentNetworkBehaviour != null)
                    {
                        nob.UnsetParent();
                        /* DespawnType also has to be updated to use default
                         * for the networkObject since this despawn is happening
                         * automatically. */
                        despawnType = nob.GetDefaultDespawnType();
                    }
                }
            }

            nob.SetIsDestroying(despawnType);
            // Deinitialize to invoke callbacks.
            nob.Deinitialize(asServer);
            // Remove from match condition only if server.
            if (asServer)
                MatchCondition.RemoveFromMatchWithoutRebuild(nob, NetworkManager);
            RemoveFromSpawned(nob, false, asServer);

            // If to destroy.
            if (destroy)
            {
                if (despawnType == DespawnType.Destroy)
                    UnityEngine.Object.Destroy(nob.gameObject);
                else
                    NetworkManager.StorePooledInstantiated(nob, asServer);
            }
            /* If to potentially disable instead of destroy.
             * This is such as something is despawning server side
             * but a clientHost is present, or if a scene object. */
            else
            {
                // If as server.
                if (asServer)
                {
                    /* If not clientHost the object can be disabled.
                     *
                     * Also, if clientHost and clientHost is not an observer, the object
                     * can be disabled. */
                    // If not clientHost then the object can be disabled.
                    if (!NetworkManager.IsClientStarted || !nob.Observers.Contains(NetworkManager.ClientManager.Connection))
                        nob.gameObject.SetActive(false);
                }
                // Not as server.
                else
                {
                    // If the server is not active then the object can be disabled.
                    if (!NetworkManager.IsServerStarted)
                    {
                        nob.gameObject.SetActive(false);
                    }
                    // If also server then checks must be done.
                    else
                    {
                        /* Object is still spawned on the server side. This means
                         * the clientHost likely lost visibility. When this is the case
                         * update clientHost renderers. */
                        if (NetworkManager.ServerManager.Objects.Spawned.ContainsKey(nob.ObjectId))
                            nob.SetRenderersVisible(false);
                        /* No longer spawned on the server, can
                         * deactivate on the client. */
                        else
                            nob.gameObject.SetActive(false);
                    }
                }

                /* Also despawn child objects.
                 * This only must be done when not destroying
                 * as destroying would result in the despawn being
                 * forced.
                 *
                 * Only run if asServer as well. The server will send
                 * individual despawns for each child. */
                if (asServer)
                {
                    List<NetworkObject> childNobs = nob.GetNetworkObjects(GetNetworkObjectOption.AllNested);
                    foreach (NetworkObject childNob in childNobs)
                    {
                        if (childNob != null && !childNob.IsDeinitializing)
                            Despawn(childNob, despawnType, asServer: true);
                    }
                }
            }
        }

        /// <summary>
        /// Initializes a prefab, not to be mistaken for initializing a spawned object.
        /// </summary>
        /// <param name = "prefab">Prefab to initialize.</param>
        /// <param name = "index">Index within spawnable prefabs.</param>
        public static void InitializePrefab(NetworkObject prefab, int index, ushort? collectionId = null)
        {
            const int invalidIndex = -1;
            if (index == invalidIndex)
            {
                Debug.LogError($"An index of {invalidIndex} cannot be assigned as a PrefabId for {prefab.name}.");
                return;
            }

            if (prefab == null)
                return;

            prefab.PrefabId = (ushort)index;
            if (collectionId != null)
                prefab.SpawnableCollectionId = collectionId.Value;

            prefab.SetInitializedValues(null, ignoreSerializedTimestamp: true);
        }

        /// <summary>
        /// Despawns Spawned NetworkObjects. Scene objects will be disabled, others will be destroyed.
        /// </summary>
        internal virtual void DespawnWithoutSynchronization(bool recursive, bool asServer)
        {
            foreach (NetworkObject nob in Spawned.Values)
            {
                if (nob == null)
                    continue;

                DespawnWithoutSynchronization(nob, recursive, asServer, nob.GetDefaultDespawnType(), removeFromSpawned: false);
            }

            ClearSpawnedCollectionAndInvoke();
        }

        /// <summary>
        /// Despawns a network object.
        /// </summary>
        /// <param name = "nob"></param>
        protected virtual void DespawnWithoutSynchronization(NetworkObject nob, bool recursive, bool asServer, DespawnType despawnType, bool removeFromSpawned)
        {
            #if FISHNET_STABLE_RECURSIVE_DESPAWNS
            recursive = false;
            #endif

            GetNetworkObjectOption getOption = recursive ? GetNetworkObjectOption.All : GetNetworkObjectOption.Self;
            List<NetworkObject> allNobs = nob.GetNetworkObjects(getOption);

            // True if can deactivate or destroy.
            bool canCleanup = asServer || !NetworkManager.IsServerStarted;

            foreach (NetworkObject lNob in allNobs)
            {
                lNob.SetIsDestroying(despawnType);
                lNob.Deinitialize(asServer);

                if (canCleanup && removeFromSpawned)
                    RemoveFromSpawned(lNob, fromOnDestroy: false, asServer);
            }

            /* Only need to check the first nob. If it's stored, deactivated,
             * or destroyed, the rest will follow. */
            if (canCleanup)
            {
                NetworkObject firstNob = allNobs[0];

                if (firstNob.IsSceneObject || firstNob.IsInitializedNested)
                {
                    firstNob.gameObject.SetActive(value: false);
                }
                else
                {
                    if (despawnType == DespawnType.Destroy)
                        UnityEngine.Object.Destroy(firstNob.gameObject);
                    else
                        NetworkManager.StorePooledInstantiated(firstNob, asServer);
                }
            }

            CollectionCaches<NetworkObject>.Store(allNobs);
        }

        /// <summary>
        /// Adds a NetworkObject to Spawned.
        /// </summary>
        internal virtual void AddToSpawned(NetworkObject nob, bool asServer)
        {
            AddtoSpawnedCollectionAndInvoke(nob);
        }

        /// <summary>
        /// Adds a NetworkObject to SceneObjects.
        /// </summary>
        protected internal void AddToSceneObjects(NetworkObject nob)
        {
            SceneObjects_Internal[nob.SceneId] = nob;
        }

        /// <summary>
        /// Removes a NetworkObject from SceneObjects.
        /// </summary>
        /// <param name = "nob"></param>
        protected internal void RemoveFromSceneObjects(NetworkObject nob)
        {
            SceneObjects_Internal.Remove(nob.SceneId);
        }

        /// <summary>
        /// Removes a NetworkObject from SceneObjects.
        /// </summary>
        /// <param name = "nob"></param>
        protected internal void RemoveFromSceneObjects(ulong sceneId)
        {
            SceneObjects_Internal.Remove(sceneId);
        }

        /// <summary>
        /// Finds a NetworkObject within Spawned.
        /// </summary>
        /// <param name = "objectId"></param>
        /// <returns></returns>
        protected internal NetworkObject GetSpawnedNetworkObject(int objectId)
        {
            NetworkObject r;
            if (!_spawned.TryGetValueIL2CPP(objectId, out r))
                NetworkManager.LogError($"Spawned NetworkObject not found for ObjectId {objectId}.");

            return r;
        }

        /// <summary>
        /// Tries to skip data length for a packet.
        /// </summary>
        /// <param name = "packetId"></param>
        /// <param name = "reader"></param>
        /// <param name = "dataLength"></param>
        protected internal void SkipDataLength(ushort packetId, PooledReader reader, int dataLength, int rpcLinkObjectId = -1)
        {
            /* -1 means length wasn't set, which would suggest a reliable packet.
             * Object should never be missing for reliable packets since spawns
             * and despawns are reliable in order. */
            if (dataLength == (int)MissingObjectPacketLength.Reliable)
            {
                string msg;
                bool isRpcLink = packetId >= NetworkManager.StartingRpcLinkIndex;
                if (isRpcLink)
                {
                    msg = rpcLinkObjectId == -1 ? $"RPCLink of Id {(PacketId)packetId} could not be found. Remaining data will be purged." : $"ObjectId {rpcLinkObjectId} for RPCLink {(PacketId)packetId} could not be found.";
                }
                else
                {
                    msg = $"NetworkBehaviour could not be found for packetId {(PacketId)packetId}. Remaining data will be purged.";
                }

                /* Default logging for server is errors only. Use error on client and warning
                 * on servers to reduce chances of allocation attacks. */
                #if DEVELOPMENT_BUILD || UNITY_EDITOR || !UNITY_SERVER
                NetworkManager.LogError(msg);
                #else
                NetworkManager.LogWarning(msg);
                #endif
                reader.Clear();
            }
            /* If length is known then is unreliable packet. It's possible
             * this packetId arrived before or after the object was spawned/destroyed.
             * Skip past the data for this packet and use rest in reader. With non-linked
             * RPCs length is sent before object information. */
            else if (dataLength >= 0)
            {
                reader.Skip(Math.Min(dataLength, reader.Remaining));
            }
            /* -2 indicates the length is very long. Don't even try saving
             * the packet, user shouldn't be sending this much data over unreliable. */
            else if (dataLength == (int)MissingObjectPacketLength.PurgeRemaiming)
            {
                reader.Clear();
            }
        }

        /// <summary>
        /// Parses a ReplicateRpc.
        /// </summary>
        internal void ParseReplicateRpc(PooledReader reader, NetworkConnection conn, Channel channel)
        {
            using (_pm_ParseReplicateRpc.Auto())
            {
                #if DEVELOPMENT
                NetworkBehaviour.ReadDebugForValidatedRpc(NetworkManager, reader, out int startReaderRemaining, out string rpcInformation, out uint expectedReadAmount);
                #endif
                int readerStartAfterDebug = reader.Position;

                NetworkBehaviour nb = reader.ReadNetworkBehaviour();
                int dataLength = Packets.GetPacketLength((ushort)PacketId.ServerRpc, reader, channel);
                if (nb != null && nb.IsSpawned)
                    nb.OnReplicateRpc(readerStartAfterDebug, hash: null, reader, conn, channel);
                else
                    SkipDataLength((ushort)PacketId.ServerRpc, reader, dataLength);

                #if DEVELOPMENT
                NetworkBehaviour.TryPrintDebugForValidatedRpc(fromRpcLink: false, NetworkManager, reader, startReaderRemaining, rpcInformation, expectedReadAmount, channel);
                #endif
            }
        }

        #if DEVELOPMENT
        /// <summary>
        /// Checks to write a scene object's details into a writer.
        /// </summary>
        protected void CheckWriteSceneObjectDetails(NetworkObject nob, Writer w)
        {
            // Check to write additional information if a scene object.
            if (NetworkManager.DebugManager.WriteSceneObjectDetails)
            {
                w.WriteString(nob.gameObject.scene.name);
                w.WriteString(nob.gameObject.name);
            }
        }

        /// <summary>
        /// Checks to read a scene object's details and populates values if read was successful.
        /// </summary>
        protected void CheckReadSceneObjectDetails(Reader r, ref string sceneName, ref string objectName)
        {
            if (NetworkManager.DebugManager.WriteSceneObjectDetails)
            {
                sceneName = r.ReadStringAllocated();
                objectName = r.ReadStringAllocated();
            }
        }
        #endif
    }
}