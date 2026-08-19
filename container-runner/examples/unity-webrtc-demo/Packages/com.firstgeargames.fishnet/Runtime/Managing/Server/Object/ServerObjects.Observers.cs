#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Connection;
using FishNet.Managing.Object;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Observing;
using FishNet.Serializing;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Managing.Server
{
    public partial class ServerObjects : ManagedObjects
    {
        #region Private.
        /// <summary>
        /// Cache filled with objects which observers are being updated.
        /// This is primarily used to invoke events after all observers are updated, rather than as each is updated.
        /// </summary>
        private List<NetworkObject> _observerChangedObjectsCache = new(100);
        /// <summary>
        /// NetworkObservers which require regularly iteration.
        /// </summary>
        private List<NetworkObject> _timedNetworkObservers = new();
        /// <summary>
        /// Index in TimedNetworkObservers to start on next cycle.
        /// </summary>
        private int _nextTimedObserversIndex;
        /// <summary>
        /// Used to write spawns for everyone. This writer will exclude owner only information.
        /// </summary>
        private PooledWriter _writer = new();
        /// <summary>
        /// Indexes within TimedNetworkObservers which are unset.
        /// </summary>
        private Queue<int> _emptiedTimedIndexes = new();
        #endregion

        /// <summary>
        /// Called when MonoBehaviours call Update.
        /// </summary>
        private void Observers_OnUpdate()
        {
            UpdateTimedObservers();
        }

        /// <summary>
        /// Progressively updates NetworkObservers with timed conditions.
        /// </summary>
        private void UpdateTimedObservers()
        {
            if (!NetworkManager.IsServerStarted)
                return;
            // No point in updating if the timemanager isn't going to tick this frame.
            if (!NetworkManager.TimeManager.FrameTicked)
                return;
            int networkObserversCount = _timedNetworkObservers.Count;
            if (networkObserversCount == 0)
                return;

            /* Try to iterate all timed observers every half a second.
             * This value will increase as there's more observers or timed conditions. */
            float timeMultiplier = 1f + (NetworkManager.ServerManager.Clients.Count * 0.005f + _timedNetworkObservers.Count * 0.0005f);
            // Check cap this way for readability.
            float completionTime = Mathf.Min(0.5f * timeMultiplier, NetworkManager.ObserverManager.MaximumTimedObserversDuration);
            uint completionTicks = NetworkManager.TimeManager.TimeToTicks(completionTime, TickRounding.RoundUp);
            /* Iterations will be the number of objects
             * to iterate to be have completed all objects by
             * the end of completionTicks. */
            int iterations = Mathf.CeilToInt((float)networkObserversCount / completionTicks);
            if (iterations > _timedNetworkObservers.Count)
                iterations = _timedNetworkObservers.Count;

            List<NetworkConnection> connCache = RetrieveAuthenticatedConnections();
            // Build nob cache.
            List<NetworkObject> nobCache = CollectionCaches<NetworkObject>.RetrieveList();
            for (int i = 0; i < iterations; i++)
            {
                if (_nextTimedObserversIndex >= _timedNetworkObservers.Count)
                    _nextTimedObserversIndex = 0;

                NetworkObject nob = _timedNetworkObservers[_nextTimedObserversIndex++];
                if (nob != null)
                    nobCache.Add(nob);
            }

            RebuildObservers(nobCache, connCache, true);

            CollectionCaches<NetworkConnection>.Store(connCache);
            CollectionCaches<NetworkObject>.Store(nobCache);
        }
        
        /// <summary>
        /// Indicates that a networkObserver component should be updated regularly. This is done automatically.
        /// </summary>
        /// <param name = "networkObject">NetworkObject to be updated.</param>
        public void AddTimedNetworkObserver(NetworkObject networkObject)
        {
            if (_emptiedTimedIndexes.TryDequeue(out int index))
                _timedNetworkObservers[index] = networkObject;
            else
                _timedNetworkObservers.Add(networkObject);
        }
        
        /// <summary>
        /// Indicates that a networkObserver component no longer needs to be updated regularly. This is done automatically.
        /// </summary>
        /// <param name = "networkObject">NetworkObject to be updated.</param>
        public void RemoveTimedNetworkObserver(NetworkObject networkObject)
        {
            int index = _timedNetworkObservers.IndexOf(networkObject);
            if (index == -1)
                return;

            _emptiedTimedIndexes.Enqueue(index);
            _timedNetworkObservers[index] = null;

            // If there's a decent amount missing then rebuild the collection.
            if (_emptiedTimedIndexes.Count > 20)
            {
                List<NetworkObject> newLst = CollectionCaches<NetworkObject>.RetrieveList();
                foreach (NetworkObject nob in _timedNetworkObservers)
                {
                    if (nob == null)
                        continue;

                    newLst.Add(nob);
                }

                CollectionCaches<NetworkObject>.Store(_timedNetworkObservers);
                _timedNetworkObservers = newLst;
                _emptiedTimedIndexes.Clear();
            }
        }

        /// <summary>
        /// Gets all NetworkConnections which are authenticated.
        /// </summary>
        /// <returns></returns>
        private List<NetworkConnection> RetrieveAuthenticatedConnections()
        {
            List<NetworkConnection> cache = CollectionCaches<NetworkConnection>.RetrieveList();
            foreach (NetworkConnection item in NetworkManager.ServerManager.Clients.Values)
            {
                if (item.IsAuthenticated)
                    cache.Add(item);
            }

            return cache;
        }

        /// <summary>
        /// Gets all spawned objects with root objects first.
        /// </summary>
        /// <returns></returns>
        private List<NetworkObject> RetrieveOrderedSpawnedObjects()
        {
            List<NetworkObject> spawnedCache = GetSpawnedNetworkObjects();

            List<NetworkObject> sortedCache = SortRootAndNestedByInitializeOrder(spawnedCache);

            CollectionCaches<NetworkObject>.Store(spawnedCache);

            return sortedCache;
        }

        /// <summary>
        /// Returns spawned NetworkObjects as a list.
        /// Collection returned is a new cache and should be disposed of properly.
        /// </summary>
        /// <returns></returns>
        private List<NetworkObject> GetSpawnedNetworkObjects()
        {
            List<NetworkObject> cache = Spawned.ValuesToList(useCache: true);

            return cache;
        }

        /// <summary>
        /// Sorts a collection of NetworkObjects root and nested by initialize order.
        /// Collection returned is a new cache and should be disposed of properly.
        /// </summary>
        internal List<NetworkObject> SortRootAndNestedByInitializeOrder(List<NetworkObject> nobs)
        {
            List<NetworkObject> sortedRootCache = CollectionCaches<NetworkObject>.RetrieveList();

            // First order root objects.
            foreach (NetworkObject item in nobs)
            {
                if (item.IsNested)
                    continue;

                sortedRootCache.AddOrdered(item);
            }

            /* After all root are ordered check
             * their nested. Order nested in segments
             * of each root then insert after the root.
             * This must be performed after all roots are ordered. */

            // This holds the results of all values.
            List<NetworkObject> sortedRootAndNestedCache = CollectionCaches<NetworkObject>.RetrieveList();

            // Cache for sorting nested.
            List<NetworkObject> sortedNestedCache = CollectionCaches<NetworkObject>.RetrieveList();

            foreach (NetworkObject item in sortedRootCache)
            {
                /* Remove recursive and only check Initialized and Runtime. Once iterated
                 * check each added entry again using Initialized and Recursive. */
                List<NetworkObject> nested = item.GetNetworkObjects(GetNetworkObjectOption.AllNestedRecursive);

                foreach (NetworkObject nestedItem in nested)
                {
                    /* If entry has a runtime nested that differs from initialized nested
                     * then it k*/
                    if (sortedNestedCache.Contains(nestedItem))
                        Debug.LogError($"Nested cache already contains item [{nestedItem.name}]. Source [{item.name}]. Please report this error.");
                    else
                        sortedNestedCache.AddOrdered(nestedItem);
                }

                CollectionCaches<NetworkObject>.Store(nested);

                /* Once all nested are sorted then can be added to the
                 * sorted root and nested cache. */
                sortedRootAndNestedCache.Add(item);
                sortedRootAndNestedCache.AddRange(sortedNestedCache);

                // Reset cache.
                sortedNestedCache.Clear();
            }

            // Store temp caches.
            CollectionCaches<NetworkObject>.Store(sortedRootCache);
            CollectionCaches<NetworkObject>.Store(sortedNestedCache);

            return sortedRootAndNestedCache;
        }

        /// <summary>
        /// Removes a connection from observers without synchronizing changes.
        /// </summary>
        /// <param name = "connection"></param>
        private void RemoveFromObserversWithoutSynchronization(NetworkConnection connection)
        {
            List<NetworkObject> observerChangedObjectsCache = _observerChangedObjectsCache;
            foreach (NetworkObject nob in Spawned.Values)
            {
                if (nob.RemoveObserver(connection))
                    observerChangedObjectsCache.Add(nob);
            }

            // Invoke despawn callbacks on nobs.
            for (int i = 0; i < observerChangedObjectsCache.Count; i++)
                observerChangedObjectsCache[i].InvokeOnServerDespawn(connection);
            observerChangedObjectsCache.Clear();
        }

        /// <summary>
        /// Rebuilds observers on all NetworkObjects for all connections.
        /// </summary>
        public void RebuildObservers(bool timedOnly = false)
        {
            List<NetworkObject> nobCache = RetrieveOrderedSpawnedObjects();
            List<NetworkConnection> connCache = RetrieveAuthenticatedConnections();

            RebuildObservers(nobCache, connCache, timedOnly);

            CollectionCaches<NetworkObject>.Store(nobCache);
            CollectionCaches<NetworkConnection>.Store(connCache);
        }

        /// <summary>
        /// Rebuilds observers for all connections for a NetworkObject.
        /// </summary>
        public void RebuildObservers(NetworkObject nob, bool timedOnly = false)
        {
            List<NetworkObject> nobCache = CollectionCaches<NetworkObject>.RetrieveList(nob);
            List<NetworkConnection> connCache = RetrieveAuthenticatedConnections();

            RebuildObservers(nobCache, connCache, timedOnly);

            CollectionCaches<NetworkObject>.Store(nobCache);
            CollectionCaches<NetworkConnection>.Store(connCache);
        }

        /// <summary>
        /// Rebuilds observers on all NetworkObjects for a connection.
        /// </summary>
        public void RebuildObservers(NetworkConnection connection, bool timedOnly = false)
        {
            List<NetworkObject> nobCache = RetrieveOrderedSpawnedObjects();
            List<NetworkConnection> connCache = CollectionCaches<NetworkConnection>.RetrieveList(connection);

            RebuildObservers(nobCache, connCache, timedOnly);

            CollectionCaches<NetworkObject>.Store(nobCache);
            CollectionCaches<NetworkConnection>.Store(connCache);
        }

        /// <summary>
        /// Rebuilds observers on NetworkObjects.
        /// </summary>
        public void RebuildObservers(IList<NetworkObject> nobs, bool timedOnly = false)
        {
            List<NetworkConnection> conns = RetrieveAuthenticatedConnections();

            RebuildObservers(nobs, conns, timedOnly);

            CollectionCaches<NetworkConnection>.Store(conns);
        }

        /// <summary>
        /// Rebuilds observers on all objects for connections.
        /// </summary>
        public void RebuildObservers(IList<NetworkConnection> connections, bool timedOnly = false)
        {
            List<NetworkObject> nobCache = RetrieveOrderedSpawnedObjects();

            RebuildObservers(nobCache, connections, timedOnly);

            CollectionCaches<NetworkObject>.Store(nobCache);
        }

        /// <summary>
        /// Rebuilds observers on NetworkObjects for connections.
        /// </summary>
        public void RebuildObservers(IList<NetworkObject> nobs, NetworkConnection conn, bool timedOnly = false)
        {
            List<NetworkConnection> connCache = CollectionCaches<NetworkConnection>.RetrieveList(conn);

            RebuildObservers(nobs, connCache, timedOnly);

            CollectionCaches<NetworkConnection>.Store(connCache);
        }

        /// <summary>
        /// Rebuilds observers for connections on NetworkObject.
        /// </summary>
        public void RebuildObservers(NetworkObject networkObject, IList<NetworkConnection> connections, bool timedOnly = false)
        {
            List<NetworkObject> nobCache = CollectionCaches<NetworkObject>.RetrieveList(networkObject);

            RebuildObservers(nobCache, connections, timedOnly);

            CollectionCaches<NetworkObject>.Store(nobCache);
        }

        /// <summary>
        /// Rebuilds observers on NetworkObjects for connections.
        /// </summary>
        public void RebuildObservers(IList<NetworkObject> nobs, IList<NetworkConnection> conns, bool timedOnly = false)
        {
            List<NetworkObject> nobCache = CollectionCaches<NetworkObject>.RetrieveList();
            NetworkConnection nc;

            int connsCount = conns.Count;
            for (int i = 0; i < connsCount; i++)
            {
                nobCache.Clear();

                nc = conns[i];
                int nobsCount = nobs.Count;
                for (int z = 0; z < nobsCount; z++)
                    RebuildObservers(nobs[z], nc, nobCache, timedOnly);

                // Send if change.
                if (_writer.Length > 0)
                {
                    NetworkManager.TransportManager.SendToClient((byte)Channel.Reliable, _writer.GetArraySegment(), nc);


                    #if DEVELOPMENT && !UNITY_SERVER
                    if (NetworkTrafficStatistics != null)
                        NetworkTrafficStatistics.AddOutboundPacketIdData(PacketId.BulkSpawnOrDespawn, string.Empty, _writer.Length, gameObject: null, asServer: true);
                    #endif

                    _writer.Clear();

                    foreach (NetworkObject n in nobCache)
                        n.OnSpawnServer(nc);
                }
            }

            CollectionCaches<NetworkObject>.Store(nobCache);
        }

        /// <summary>
        /// Rebuilds observers for a connection on NetworkObject.
        /// </summary>
        public void RebuildObservers(NetworkObject nob, NetworkConnection conn, bool timedOnly = false)
        {
            if (ApplicationState.IsQuitting())
                return;
            _writer.Clear();

            #if DEVELOPMENT && !UNITY_SERVER
            PacketId trafficPacketId;
            #endif
            conn.UpdateHashGridPositions(!timedOnly);
            // If observer state changed then write changes.
            ObserverStateChange osc = nob.RebuildObservers(conn, timedOnly);
            if (osc == ObserverStateChange.Added)
            {
                WriteSpawn(nob, _writer, conn);
                #if DEVELOPMENT && !UNITY_SERVER
                trafficPacketId = PacketId.ObjectSpawn;
                #endif
            }
            else if (osc == ObserverStateChange.Removed)
            {
                nob.InvokeOnServerDespawn(conn);
                WriteDespawn(nob, nob.GetDefaultDespawnType(), _writer);
                #if DEVELOPMENT && !UNITY_SERVER
                trafficPacketId = PacketId.ObjectDespawn;
                #endif
            }
            else
            {
                return;
            }

            #if DEVELOPMENT && !UNITY_SERVER
            if (NetworkTrafficStatistics != null)
                NetworkTrafficStatistics.AddOutboundPacketIdData(trafficPacketId, string.Empty, _writer.Length, gameObject: null, asServer: true);
            #endif

            NetworkManager.TransportManager.SendToClient((byte)Channel.Reliable, _writer.GetArraySegment(), conn);

            /* If spawning then also invoke server
             * start events, such as buffer last
             * and onspawnserver. */
            if (osc == ObserverStateChange.Added)
                nob.OnSpawnServer(conn);

            _writer.Clear();

            /* If there is change then also rebuild recursive networkObjects. */
            foreach (NetworkBehaviour item in nob.RuntimeChildNetworkBehaviours)
                RebuildObservers(item.NetworkObject, conn, timedOnly);
        }

        /// <summary>
        /// Rebuilds observers for a connection on NetworkObject.
        /// </summary>
        internal void RebuildObservers(NetworkObject nob, NetworkConnection conn, List<NetworkObject> addedNobs, bool timedOnly = false)
        {
            if (ApplicationState.IsQuitting())
                return;

            /* When not using a timed rebuild such as this connections must have
             * hashgrid data rebuilt immediately. */
            conn.UpdateHashGridPositions(!timedOnly);

            // If observer state changed then write changes.
            ObserverStateChange osc = nob.RebuildObservers(conn, timedOnly);
            if (osc == ObserverStateChange.Added)
            {
                WriteSpawn(nob, _writer, conn);
                addedNobs.Add(nob);
            }
            else if (osc == ObserverStateChange.Removed)
            {
                nob.InvokeOnServerDespawn(conn);
                WriteDespawn(nob, nob.GetDefaultDespawnType(), _writer);
            }
            else
            {
                return;
            }

            /* If there is change then also rebuild on any runtime children.
             * This is to ensure runtime children have visibility updated
             * in relation to parent.
             *
             * If here there is change. */
            foreach (NetworkBehaviour item in nob.RuntimeChildNetworkBehaviours)
                RebuildObservers(item.NetworkObject, conn, addedNobs, timedOnly);
        }
    }
}