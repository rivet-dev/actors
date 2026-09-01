using FishNet.Connection;
using FishNet.Managing;
using FishNet.Managing.Logging;
using FishNet.Managing.Server;
using FishNet.Object;
using FishNet.Observing;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Component.Observing
{
    /// <summary>
    /// When this observer condition is placed on an object, a client must be within the same match to view the object.
    /// </summary>
    [CreateAssetMenu(menuName = "FishNet/Observers/Match Condition", fileName = "New Match Condition")]
    public class MatchCondition : ObserverCondition
    {
        #region Types.
        /// <summary>
        /// MatchCondition collections used.
        /// </summary>
        public class ConditionCollections
        {
            public Dictionary<int, HashSet<NetworkConnection>> MatchConnections = new();
            public Dictionary<NetworkConnection, HashSet<int>> ConnectionMatches = new();
            public Dictionary<int, HashSet<NetworkObject>> MatchObjects = new();
            public Dictionary<NetworkObject, HashSet<int>> ObjectMatches = new();
        }
        #endregion

        #region Private.
        /// <summary>
        /// Collections for each NetworkManager instance.
        /// </summary>
        private static Dictionary<NetworkManager, ConditionCollections> _collections = new();
        #endregion

        #region Collections.
        /// <summary>
        /// Stores collections for a manager.
        /// </summary>
        /// <param name = "manager"></param>
        internal static void StoreCollections(NetworkManager manager)
        {
            ConditionCollections cc;
            if (!_collections.TryGetValue(manager, out cc))
                return;

            foreach (HashSet<int> item in cc.ObjectMatches.Values)
                CollectionCaches<int>.Store(item);
            foreach (HashSet<NetworkConnection> item in cc.MatchConnections.Values)
                CollectionCaches<NetworkConnection>.Store(item);
            foreach (HashSet<NetworkObject> item in cc.MatchObjects.Values)
                CollectionCaches<NetworkObject>.Store(item);
            foreach (HashSet<int> item in cc.ConnectionMatches.Values)
                CollectionCaches<int>.Store(item);

            _collections.Remove(manager);
        }

        /// <summary>
        /// Gets condition collections for a NetowrkManager.
        /// </summary>
        private static ConditionCollections GetCollections(NetworkManager manager = null)
        {
            if (manager == null)
                manager = InstanceFinder.NetworkManager;

            ConditionCollections cc;
            if (!_collections.TryGetValue(manager, out cc))
            {
                cc = new();
                _collections[manager] = cc;
            }

            return cc;
        }

        /// <summary>
        /// Returns matches and connections in each match.
        /// </summary>
        /// <param name = "manager">NetworkManager to use.</param>
        /// <returns></returns>
        public static Dictionary<int, HashSet<NetworkConnection>> GetMatchConnections(NetworkManager manager = null)
        {
            ConditionCollections cc = GetCollections(manager);
            return cc.MatchConnections;
        }

        /// <summary>
        /// Returns connections and the matches they are in.
        /// </summary>
        /// <param name = "manager">NetworkManager to use.</param>
        /// <returns></returns>
        public static Dictionary<NetworkConnection, HashSet<int>> GetConnectionMatches(NetworkManager manager = null)
        {
            ConditionCollections cc = GetCollections(manager);
            return cc.ConnectionMatches;
        }

        /// <summary>
        /// Returns matches and objects within each match.
        /// </summary>
        /// <param name = "manager">NetworkManager to use.</param>
        /// <returns></returns>
        public static Dictionary<int, HashSet<NetworkObject>> GetMatchObjects(NetworkManager manager = null)
        {
            ConditionCollections cc = GetCollections(manager);
            return cc.MatchObjects;
        }

        /// <summary>
        /// Returns objects and the matches they are in.
        /// </summary>
        /// <param name = "manager">NetworkManager to use.</param>
        /// <returns></returns>
        public static Dictionary<NetworkObject, HashSet<int>> GetObjectMatches(NetworkManager manager = null)
        {
            ConditionCollections cc = GetCollections(manager);
            return cc.ObjectMatches;
        }
        #endregion

        #region Add to match NetworkConnection.
        /// <summary>
        /// Adds a connection to a match.
        /// </summary>
        private static bool AddToMatch(int match, NetworkConnection conn, NetworkManager manager, bool replaceMatch, bool rebuild)
        {
            Dictionary<int, HashSet<NetworkConnection>> matchConnections = GetMatchConnections(manager);

            if (replaceMatch)
                RemoveFromMatchesWithoutRebuild(conn, manager);

            /* Get current connections in match. This is where the conn
             * will be added to. If does not exist then make new
             * collection. */
            HashSet<NetworkConnection> matchConnValues;
            if (!matchConnections.TryGetValueIL2CPP(match, out matchConnValues))
            {
                matchConnValues = CollectionCaches<NetworkConnection>.RetrieveHashSet();
                matchConnections.Add(match, matchConnValues);
            }

            bool r = matchConnValues.Add(conn);
            AddToConnectionMatches(conn, match, manager);
            if (r && rebuild)
                GetServerObjects(manager).RebuildObservers();

            return r;
        }

        /// <summary>
        /// Adds a connection to a match.
        /// </summary>
        /// <param name = "match">Match to add conn to.</param>
        /// <param name = "conn">Connection to add to match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        /// <param name = "replaceMatch">True to replace other matches with the new match.</param>
        public static void AddToMatch(int match, NetworkConnection conn, NetworkManager manager = null, bool replaceMatch = false)
        {
            AddToMatch(match, conn, manager, replaceMatch, true);
        }

        /// <summary>
        /// Updates a connection within ConnectionMatches to contain match.
        /// </summary>
        private static void AddToConnectionMatches(NetworkConnection conn, int match, NetworkManager manager)
        {
            Dictionary<NetworkConnection, HashSet<int>> connectionMatches = GetConnectionMatches(manager);

            HashSet<int> matches;
            if (!connectionMatches.TryGetValueIL2CPP(conn, out matches))
            {
                matches = CollectionCaches<int>.RetrieveHashSet();
                connectionMatches[conn] = matches;
            }

            matches.Add(match);
        }

        /// <summary>
        /// Adds connections to a match.
        /// </summary>
        /// <param name = "match">Match to add conns to.</param>
        /// <param name = "conns">Connections to add to match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        /// <param name = "replaceMatch">True to replace other matches with the new match.</param>
        public static void AddToMatch(int match, NetworkConnection[] conns, NetworkManager manager = null, bool replaceMatch = false)
        {
            AddToMatch(match, conns.ToList(), manager, replaceMatch);
        }

        /// <summary>
        /// Adds connections to a match.
        /// </summary>
        /// <param name = "match">Match to add conns to.</param>
        /// <param name = "conns">Connections to add to match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        /// <param name = "replaceMatch">True to replace other matches with the new match.</param>
        public static void AddToMatch(int match, List<NetworkConnection> conns, NetworkManager manager = null, bool replaceMatch = false)
        {
            bool added = false;
            foreach (NetworkConnection c in conns)
                added |= AddToMatch(match, c, manager, replaceMatch, false);

            if (added)
                GetServerObjects(manager).RebuildObservers();
        }
        #endregion

        #region Add to match NetworkObject.
        /// <summary>
        /// Adds an object to a match.
        /// </summary>
        private static bool AddToMatch(int match, NetworkObject nob, NetworkManager manager, bool replaceMatch, bool rebuild)
        {
            Dictionary<int, HashSet<NetworkObject>> matchObjects = GetMatchObjects(manager);
            Dictionary<NetworkObject, HashSet<int>> objectMatches = GetObjectMatches(manager);

            if (replaceMatch)
                RemoveFromMatchWithoutRebuild(nob, manager);

            HashSet<NetworkObject> matchObjectsValues;
            if (!matchObjects.TryGetValueIL2CPP(match, out matchObjectsValues))
            {
                matchObjectsValues = CollectionCaches<NetworkObject>.RetrieveHashSet();
                matchObjects.Add(match, matchObjectsValues);
            }
            bool added = matchObjectsValues.Add(nob);

            /* Also add to reverse dictionary. */
            HashSet<int> objectMatchesValues;
            if (!objectMatches.TryGetValueIL2CPP(nob, out objectMatchesValues))
            {
                objectMatchesValues = CollectionCaches<int>.RetrieveHashSet();
                objectMatches.Add(nob, objectMatchesValues);
            }
            objectMatchesValues.Add(match);

            if (added && rebuild)
                GetServerObjects(manager).RebuildObservers();

            return added;
        }

        /// <summary>
        /// Adds an object to a match.
        /// </summary>
        /// <param name = "match">Match to add conn to.</param>
        /// <param name = "nob">Connection to add to match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        /// <param name = "replaceMatch">True to replace other matches with the new match.</param>
        public static void AddToMatch(int match, NetworkObject nob, NetworkManager manager = null, bool replaceMatch = false)
        {
            AddToMatch(match, nob, manager, replaceMatch, true);
        }

        /// <summary>
        /// Adds objects to a match.
        /// </summary>
        /// <param name = "match">Match to add conns to.</param>
        /// <param name = "nobs">Connections to add to match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        /// <param name = "replaceMatch">True to replace other matches with the new match.</param>
        public static void AddToMatch(int match, NetworkObject[] nobs, NetworkManager manager = null, bool replaceMatch = false)
        {
            AddToMatch(match, nobs.ToList(), manager, replaceMatch);
        }

        /// <summary>
        /// Adds objects to a match.
        /// </summary>
        /// <param name = "match">Match to add conns to.</param>
        /// <param name = "nobs">Connections to add to match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        /// <param name = "replaceMatch">True to replace other matches with the new match.</param>
        public static void AddToMatch(int match, List<NetworkObject> nobs, NetworkManager manager = null, bool replaceMatch = false)
        {
            // Remove from current matches.
            if (replaceMatch)
            {
                foreach (NetworkObject n in nobs)
                    RemoveFromMatchWithoutRebuild(n, manager);
            }

            bool added = false;
            // Add to matches.
            foreach (NetworkObject n in nobs)
                added |= AddToMatch(match, n, manager, replaceMatch, false);

            if (added)
                GetServerObjects(manager).RebuildObservers();
        }
        #endregion

        #region TryRemoveKey.
        /// <summary>
        /// Removes a key if values are empty, and caches values.
        /// </summary>
        private static void TryRemoveKey(Dictionary<int, HashSet<NetworkObject>> dict, int key, HashSet<NetworkObject> value)
        {
            bool isEmpty = true;
            if (value != null)
            {
                isEmpty = value.Count == 0;
                if (isEmpty)
                    CollectionCaches<NetworkObject>.Store(value);
            }

            if (isEmpty)
                dict.Remove(key);
        }

        /// <summary>
        /// Removes a key if values are empty, and caches values.
        /// </summary>
        private static void TryRemoveKey(Dictionary<int, HashSet<NetworkObject>> dict, int key)
        {
            HashSet<NetworkObject> value;
            dict.TryGetValue(key, out value);
            TryRemoveKey(dict, key, value);
        }

        /// <summary>
        /// Removes a key if values are empty, and caches values.
        /// </summary>
        private static void TryRemoveKey(Dictionary<NetworkObject, HashSet<int>> dict, NetworkObject key, HashSet<int> value)
        {
            bool isEmpty = true;
            if (value != null)
            {
                isEmpty = value.Count == 0;
                if (isEmpty)
                    CollectionCaches<int>.Store(value);
            }

            if (isEmpty)
                dict.Remove(key);
        }

        /// <summary>
        /// Removes a key if values are empty, and caches values.
        /// </summary>
        private static void TryRemoveKey(Dictionary<NetworkObject, HashSet<int>> dict, NetworkObject key)
        {
            HashSet<int> value;
            dict.TryGetValueIL2CPP(key, out value);
            TryRemoveKey(dict, key, value);
        }

        /// <summary>
        /// Removes a key if values are empty, and caches values.
        /// </summary>
        private static void TryRemoveKey(Dictionary<int, HashSet<NetworkConnection>> dict, int key, HashSet<NetworkConnection> value)
        {
            bool isEmpty = true;
            if (value != null)
            {
                isEmpty = value.Count == 0;
                if (isEmpty)
                    CollectionCaches<NetworkConnection>.Store(value);
            }

            if (isEmpty)
                dict.Remove(key);
        }

        /// <summary>
        /// Removes a key if values are empty, and caches values.
        /// </summary>
        private static void TryRemoveKey(Dictionary<int, HashSet<NetworkConnection>> dict, int key)
        {
            HashSet<NetworkConnection> value;
            dict.TryGetValueIL2CPP(key, out value);
            TryRemoveKey(dict, key, value);
        }

        /// <summary>
        /// Removes a key if values are empty, and caches values.
        /// </summary>
        private static void TryRemoveKey(Dictionary<NetworkConnection, HashSet<int>> dict, NetworkConnection key, HashSet<int> value)
        {
            bool isEmpty = true;
            if (value != null)
            {
                isEmpty = value.Count == 0;
                if (isEmpty)
                    CollectionCaches<int>.Store(value);
            }

            if (isEmpty)
                dict.Remove(key);
        }

        /// <summary>
        /// Removes a key and caches collections where needed.
        /// </summary>
        private static void TryRemoveKey(Dictionary<NetworkConnection, HashSet<int>> dict, NetworkConnection key)
        {
            HashSet<int> value;
            dict.TryGetValueIL2CPP(key, out value);
            TryRemoveKey(dict, key, value);
        }
        #endregion

        #region Remove from match NetworkConnection.
        /// <summary>
        /// Removes a connection from all matches without rebuilding observers.
        /// </summary>
        /// <param name = "conn">Connection to remove from matches.</param>
        /// <param name = "manager">NetworkManager connection belongs to. This is not currently used.</param>
        internal static bool RemoveFromMatchesWithoutRebuild(NetworkConnection conn, NetworkManager manager)
        {
            Dictionary<NetworkConnection, HashSet<int>> connectionMatches = GetConnectionMatches(manager);
            Dictionary<int, HashSet<NetworkConnection>> matchConnections = GetMatchConnections(manager);

            bool removed = false;
            // If found to be in a match.
            if (connectionMatches.TryGetValueIL2CPP(conn, out HashSet<int> connectionMatchesValues))
            {
                removed = connectionMatchesValues.Count > 0;
                foreach (int m in connectionMatchesValues)
                {
                    HashSet<NetworkConnection> matchConnsValues;
                    // If match is found.
                    if (matchConnections.TryGetValue(m, out matchConnsValues))
                    {
                        matchConnsValues.Remove(conn);
                        TryRemoveKey(matchConnections, m, matchConnsValues);
                    }
                }

                // Clear matches connection is in.
                connectionMatchesValues.Clear();
                // Remove from connectionMatches.
                TryRemoveKey(connectionMatches, conn, connectionMatchesValues);
            }

            return removed;
        }

        /// <summary>
        /// Removes a connection from all matches.
        /// </summary>
        /// <param name = "conn">NetworkConnection to remove.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(NetworkConnection conn, NetworkManager manager)
        {
            bool removed = RemoveFromMatchesWithoutRebuild(conn, manager);
            if (removed)
                GetServerObjects(manager).RebuildObservers();
        }

        /// <summary>
        /// Removes a connection from a match.
        /// </summary>
        private static bool RemoveFromMatch(int match, NetworkConnection conn, NetworkManager manager, bool rebuild)
        {
            Dictionary<NetworkConnection, HashSet<int>> connectionMatches = GetConnectionMatches(manager);
            Dictionary<int, HashSet<NetworkConnection>> matchConnections = GetMatchConnections(manager);

            bool removed = false;
            HashSet<NetworkConnection> matchConnsValues;
            if (matchConnections.TryGetValueIL2CPP(match, out matchConnsValues))
            {
                removed |= matchConnsValues.Remove(conn);
                HashSet<int> connectionMatchesValues;
                if (connectionMatches.TryGetValueIL2CPP(conn, out connectionMatchesValues))
                {
                    connectionMatchesValues.Remove(match);
                    TryRemoveKey(connectionMatches, conn, connectionMatchesValues);
                }
                if (removed && rebuild)
                {
                    TryRemoveKey(matchConnections, match, matchConnsValues);
                    GetServerObjects(manager).RebuildObservers();
                }
            }

            return removed;
        }

        /// <summary>
        /// Removes a connection from a match.
        /// </summary>
        /// <param name = "match">Match to remove conn from.</param>
        /// <param name = "conn">Connection to remove from match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static bool RemoveFromMatch(int match, NetworkConnection conn, NetworkManager manager = null)
        {
            return RemoveFromMatch(match, conn, manager, true);
        }

        /// <summary>
        /// Removes connections from a match.
        /// </summary>
        /// <param name = "match">Match to remove conns from.</param>
        /// <param name = "conns">Connections to remove from match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(int match, NetworkConnection[] conns, NetworkManager manager)
        {
            RemoveFromMatch(match, conns.ToList(), manager);
        }

        /// <summary>
        /// Removes connections from a match.
        /// </summary>
        /// <param name = "match">Match to remove conns from.</param>
        /// <param name = "conns">Connections to remove from match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(int match, List<NetworkConnection> conns, NetworkManager manager)
        {
            bool removed = false;
            foreach (NetworkConnection c in conns)
                removed |= RemoveFromMatch(match, c, manager, false);

            if (removed)
                GetServerObjects(manager).RebuildObservers();
        }
        #endregion

        #region Remove from match NetworkObject.
        /// <summary>
        /// Removes a network object from any match without rebuilding observers.
        /// </summary>
        /// <param name = "nob">NetworkObject to remove.</param>
        /// <param name = "manager">Manager which the network object belongs to. This value is not yet used.</param>
        internal static bool RemoveFromMatchWithoutRebuild(NetworkObject nob, NetworkManager manager)
        {
            Dictionary<NetworkObject, HashSet<int>> objectMatches = GetObjectMatches(manager);
            Dictionary<int, HashSet<NetworkObject>> matchObjects = GetMatchObjects(manager);

            HashSet<int> objectMatchesValues;
            bool removed = false;
            // If found to be in a match.
            if (objectMatches.TryGetValueIL2CPP(nob, out objectMatchesValues))
            {
                removed = objectMatchesValues.Count > 0;
                foreach (int m in objectMatchesValues)
                {
                    // If match is found.
                    if (matchObjects.TryGetValue(m, out HashSet<NetworkObject> matchObjectsValues))
                    {
                        matchObjectsValues.Remove(nob);
                        TryRemoveKey(matchObjects, m, matchObjectsValues);
                    }
                }

                // Since object is being removed from all matches this can be cleared.
                objectMatchesValues.Clear();
                TryRemoveKey(objectMatches, nob, objectMatchesValues);
            }

            return removed;
        }

        /// <summary>
        /// Removes nob from all matches.
        /// </summary>
        /// <param name = "nob">NetworkObject to remove.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static bool RemoveFromMatch(NetworkObject nob, NetworkManager manager = null)
        {
            bool removed = RemoveFromMatchWithoutRebuild(nob, manager);
            if (removed)
                GetServerObjects(manager).RebuildObservers(nob);

            return removed;
        }

        /// <summary>
        /// Removes a network object from all matches.
        /// </summary>
        /// <param name = "nobs">NetworkObjects to remove.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(NetworkObject[] nobs, NetworkManager manager = null)
        {
            RemoveFromMatch(nobs.ToList(), manager);
        }

        /// <summary>
        /// Removes network objects from all matches.
        /// </summary>
        /// <param name = "nobs">NetworkObjects to remove.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(List<NetworkObject> nobs, NetworkManager manager = null)
        {
            bool removed = false;
            foreach (NetworkObject n in nobs)
                removed |= RemoveFromMatchWithoutRebuild(n, manager);

            if (removed)
                GetServerObjects(manager).RebuildObservers(nobs);
        }

        /// <summary>
        /// Removes a network object from a match.
        /// </summary>
        /// <param name = "match">Match to remove conn from.</param>
        /// <param name = "nob">NetworkObject to remove from match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(int match, NetworkObject nob, NetworkManager manager = null)
        {
            Dictionary<int, HashSet<NetworkObject>> matchObjects = GetMatchObjects(manager);
            Dictionary<NetworkObject, HashSet<int>> objectMatches = GetObjectMatches(manager);

            HashSet<NetworkObject> matchObjectsValues;
            if (matchObjects.TryGetValueIL2CPP(match, out matchObjectsValues))
            {
                bool removed = matchObjectsValues.Remove(nob);

                if (removed)
                {
                    /* Check if nob is still in matches. If not then remove
                     * nob from ObjectMatches. */
                    HashSet<int> objectMatchesValues;
                    if (objectMatches.TryGetValueIL2CPP(nob, out objectMatchesValues))
                    {
                        objectMatchesValues.Remove(match);
                        TryRemoveKey(objectMatches, nob, objectMatchesValues);
                    }

                    TryRemoveKey(matchObjects, match, matchObjectsValues);
                    GetServerObjects(manager).RebuildObservers(nob);
                }
            }
        }

        /// <summary>
        /// Removes network objects from a match.
        /// </summary>
        /// <param name = "match">Match to remove conns from.</param>
        /// <param name = "nobs">NetworkObjects to remove from match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(int match, NetworkObject[] nobs, NetworkManager manager = null)
        {
            Dictionary<int, HashSet<NetworkObject>> matchObjects = GetMatchObjects(manager);
            Dictionary<NetworkObject, HashSet<int>> objectMatches = GetObjectMatches(manager);

            if (matchObjects.TryGetValueIL2CPP(match, out HashSet<NetworkObject> matchObjectsValues))
            {
                bool removed = false;
                for (int i = 0; i < nobs.Length; i++)
                {
                    NetworkObject n = nobs[i];
                    removed |= matchObjectsValues.Remove(n);
                    objectMatches.Remove(n);
                }

                if (removed)
                {
                    TryRemoveKey(matchObjects, match, matchObjectsValues);
                    GetServerObjects(manager).RebuildObservers(nobs);
                }
            }
        }

        /// <summary>
        /// Removes network objects from a match.
        /// </summary>
        /// <param name = "match">Match to remove conns from.</param>
        /// <param name = "nobs">NetworkObjects to remove from match.</param>
        /// <param name = "manager">NetworkManager to rebuild observers on. If null InstanceFinder.NetworkManager will be used.</param>
        public static void RemoveFromMatch(int match, List<NetworkObject> nobs, NetworkManager manager = null)
        {
            Dictionary<int, HashSet<NetworkObject>> matchObjects = GetMatchObjects(manager);
            Dictionary<NetworkObject, HashSet<int>> objectMatches = GetObjectMatches(manager);

            if (matchObjects.TryGetValueIL2CPP(match, out HashSet<NetworkObject> matchObjectsValues))
            {
                bool removed = false;
                for (int i = 0; i < nobs.Count; i++)
                {
                    NetworkObject n = nobs[i];
                    removed |= matchObjectsValues.Remove(n);
                    objectMatches.Remove(n);
                }

                if (removed)
                {
                    TryRemoveKey(matchObjects, match, matchObjectsValues);
                    GetServerObjects(manager).RebuildObservers(nobs);
                }
            }
        }
        #endregion

        /// <summary>
        /// Returns if the object which this condition resides should be visible to connection.
        /// </summary>
        /// <param name = "connection">Connection which the condition is being checked for.</param>
        /// <param name = "currentlyAdded">True if the connection currently has visibility of this object.</param>
        /// <param name = "notProcessed">True if the condition was not processed. This can be used to skip processing for performance. While output as true this condition result assumes the previous ConditionMet value.</param>
        public override bool ConditionMet(NetworkConnection connection, bool currentlyAdded, out bool notProcessed)
        {
            // If here then checks are being processed.
            notProcessed = false;
            NetworkConnection owner = NetworkObject.Owner;
            /* If object is owned then check if owner
             * and connection share a match. */
            if (owner.IsValid)
            {
                Dictionary<NetworkConnection, HashSet<int>> connectionMatches = GetConnectionMatches(NetworkObject.NetworkManager);
                // Output owner matches.
                HashSet<int> ownerMatches;
                /* This objects owner is not in a match so treat it like
                 * a networkobject without an owner. Objects not in matches
                 * are visible to everyone. */
                if (!connectionMatches.TryGetValueIL2CPP(owner, out ownerMatches))
                {
                    return true;
                }
                /* Owner is in a match. See if connection is in any of
                 * the same matches. */
                else
                {
                    // If conn is not in any matches then they cannot see this object, as it is.
                    if (!connectionMatches.TryGetValue(connection, out HashSet<int> connMatches))
                    {
                        return false;
                    }
                    // See if conn is in any of the same matches.
                    else
                    {
                        foreach (int m in connMatches)
                        {
                            if (ownerMatches.Contains(m))
                                return true;
                        }
                    }

                    // Fall through, not found.
                    return false;
                }
            }
            /* If no owner see if the object is in a match and if so
             * then compare that. */
            else
            {
                Dictionary<NetworkObject, HashSet<int>> objectMatches = GetObjectMatches(NetworkObject.NetworkManager);
                Dictionary<NetworkConnection, HashSet<int>> connectionMatches = GetConnectionMatches(NetworkObject.NetworkManager);

                // Object isn't in a match. Is visible with no owner.
                HashSet<int> objectMatchesValues;
                if (!objectMatches.TryGetValueIL2CPP(NetworkObject, out objectMatchesValues))
                    return true;
                /* See if connection is in any of same matches as the object.
                 * If connection isn't in a match then it fails as at this point
                 * object would be, but not conn. */
                if (!connectionMatches.TryGetValueIL2CPP(connection, out HashSet<int> connectionMatchesValues))
                    return false;

                // Compare for same matches.
                foreach (int cM in connectionMatchesValues)
                {
                    if (objectMatchesValues.Contains(cM))
                        return true;
                }

                // Fall through, not in any of the matches.
                return false;
            }
        }

        /// <summary>
        /// Returns which ServerObjects to rebuild observers on.
        /// </summary>
        /// <param name = "nm"></param>
        /// <returns></returns>
        private static ServerObjects GetServerObjects(NetworkManager manager)
        {
            return manager == null ? InstanceFinder.ServerManager.Objects : manager.ServerManager.Objects;
        }

        /// <summary>
        /// How a condition is handled.
        /// </summary>
        /// <returns></returns>
        public override ObserverConditionType GetConditionType() => ObserverConditionType.Normal;
    }
}