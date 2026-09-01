using FishNet.Managing;
using FishNet.Managing.Object;
using FishNet.Object;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Utility.Performance
{
    public class DefaultObjectPool : ObjectPool
    {
        #region Public.
        /// <summary>
        /// Cache for pooled NetworkObjects.
        /// Key: CollectionId.
        /// </summary>
        public IReadOnlyList<Dictionary<int, Stack<NetworkObject>>> Cache => _cache;
        private List<Dictionary<int, Stack<NetworkObject>>> _cache = new();
        #endregion

        #region Serialized.
        /// <summary>
        /// True if to use object pooling.
        /// </summary>
        [Tooltip("True if to use object pooling.")]
        [SerializeField]
        private bool _enabled = true;
        #endregion

        #region Private.
        /// <summary>
        /// Current count of the cache collection.
        /// </summary>
        private int _cacheCount = 0;
        #endregion

#pragma warning disable CS0672 // Member overrides obsolete member
        public override NetworkObject RetrieveObject(int prefabId, ushort collectionId, Transform parent = null, Vector3? nullablePosition = null, Quaternion? nullableRotation = null, Vector3? nullableScale = null, bool makeActive = true, bool asServer = true)
#pragma warning restore CS0672 // Member overrides obsolete member
        {
            ObjectPoolRetrieveOption options = ObjectPoolRetrieveOption.Unset;
            if (makeActive)
                options |= ObjectPoolRetrieveOption.MakeActive;

            return RetrieveObject(prefabId, collectionId, options, parent, nullablePosition, nullableRotation, nullableScale, asServer);
        }

        /// <summary>
        /// Returns an object that has been stored. A new object will be created if no stored objects are available.
        /// </summary>
        /// <param name = "prefabId">PrefabId of the object to return.</param>
        /// <param name = "collectionId">CollectionId of the object to return.</param>
        /// <param name = "asServer">True if being called on the server side.</param>
        /// <returns></returns>
        public override NetworkObject RetrieveObject(int prefabId, ushort collectionId, ObjectPoolRetrieveOption options, Transform parent = null, Vector3? nullablePosition = null, Quaternion? nullableRotation = null, Vector3? nullableScale = null, bool asServer = true)
        {
            bool makeActive = options.FastContains(ObjectPoolRetrieveOption.MakeActive);
            bool localSpace = options.FastContains(ObjectPoolRetrieveOption.LocalSpace);

            if (!_enabled)
                return GetFromInstantiate();

            Stack<NetworkObject> cache = GetCache(collectionId, prefabId, createIfMissing: true);
            NetworkObject nob = null;

            // Iterate until nob is populated just in case cache entries have been destroyed.
            while (nob == null)
            {
                if (cache.TryPop(out nob))
                {
                    if (nob != null)
                    {
                        nob.transform.SetParent(parent);
                        if (localSpace)
                            nob.transform.SetLocalPositionRotationAndScale(nullablePosition, nullableRotation, nullableScale);
                        else
                            nob.transform.SetWorldPositionRotationAndScale(nullablePosition, nullableRotation, nullableScale);

                        if (makeActive)
                            nob.gameObject.SetActive(true);

                        return nob;
                    }
                }
                // Nothing left in cache.
                else
                {
                    break;
                }
            }

            // Fall through, nothing in cache.
            return GetFromInstantiate();

            // Returns a network object via instantation.
            NetworkObject GetFromInstantiate()
            {
                NetworkObject prefab = GetPrefab(prefabId, collectionId, asServer);
                if (prefab == null)
                {
                    return null;
                }
                else
                {
                    NetworkObject result;
                    Vector3 scale;

                    if (localSpace)
                    {
                        prefab.transform.OutLocalPropertyValues(nullablePosition, nullableRotation, nullableScale, out Vector3 pos, out Quaternion rot, out scale);
                        if (parent != null)
                        {
                            // Convert pos and rot to world values for the instantiate.
                            pos = parent.TransformPoint(pos);
                            rot = parent.rotation * rot;
                        }
                        result = Instantiate(prefab, pos, rot, parent);
                    }
                    else
                    {
                        prefab.transform.OutWorldPropertyValues(nullablePosition, nullableRotation, nullableScale, out Vector3 pos, out Quaternion rot, out scale);
                        result = Instantiate(prefab, pos, rot, parent);
                    }

                    result.transform.localScale = scale;

                    if (makeActive)
                        result.gameObject.SetActive(true);
                    return result;
                }
            }
        }

        /// <summary>
        /// Returns a prefab for prefab and collectionId.
        /// </summary>
        public override NetworkObject GetPrefab(int prefabId, ushort collectionId, bool asServer)
        {
            PrefabObjects po = NetworkManager.GetPrefabObjects<PrefabObjects>(collectionId, false);
            return po.GetObject(asServer, prefabId);
        }

        /// <summary>
        /// Stores an object into the pool.
        /// </summary>
        /// <param name = "instantiated">Object to store.</param>
        /// <param name = "asServer">True if being called on the server side.</param>
        /// <returns></returns>
        public override void StoreObject(NetworkObject instantiated, bool asServer)
        {
            // Pooling is not enabled.
            if (!_enabled)
            {
                Destroy(instantiated.gameObject);
                return;
            }
            
            /* This call occurs here rather than in the object pool
             * to protect the user should they be using a
             * custom pool. */
            SetTransformToSerializedValues(instantiated);

            // Get all children as well and reset state on them.
            List<NetworkObject> nestedNobs = instantiated.GetNetworkObjects(GetNetworkObjectOption.All);

            foreach (NetworkObject nob in nestedNobs)
                nob.ResetState(asServer);

            CollectionCaches<NetworkObject>.Store(nestedNobs);

            // Set root inactive.
            instantiated.gameObject.SetActive(false);

            Stack<NetworkObject> cache = GetCache(instantiated.SpawnableCollectionId, instantiated.PrefabId, createIfMissing: true);
            cache.Push(instantiated);
        }

        /// <summary>
        /// Instantiates a number of objects and adds them to the pool.
        /// </summary>
        /// <param name = "prefab">Prefab to cache.</param>
        /// <param name = "count">Quantity to spawn.</param>
        /// <param name = "asServer">True if storing prefabs for the server collection. This is only applicable when using DualPrefabObjects.</param>
#pragma warning disable CS0672 // Member overrides obsolete member
        public override void CacheObjects(NetworkObject prefab, int count, bool asServer) => StorePrefabObjects(prefab, count, asServer);
#pragma warning restore CS0672 // Member overrides obsolete member

        /// <summary>
        /// Instantiates a number of objects and adds them to the pool.
        /// </summary>
        /// <param name = "prefab">Prefab to cache.</param>
        /// <param name = "count">Quantity to spawn.</param>
        /// <param name = "asServer">True if storing prefabs for the server collection. This is only applicable when using DualPrefabObjects.</param>
        /// <returns>Prefabs instantiated and added to cache.</returns>
        public override List<NetworkObject> StorePrefabObjects(NetworkObject prefab, int count, bool asServer)
        {
            if (!_enabled)
                return null;
            if (count <= 0)
                return null;
            if (prefab == null)
                return null;
            if (prefab.PrefabId == NetworkObject.UNSET_PREFABID_VALUE)
            {
                NetworkManager.LogError($"Pefab {prefab.name} has an invalid prefabId and cannot be cached.");
                return null;
            }

            List<NetworkObject> added = new();
            Stack<NetworkObject> cache = GetCache(prefab.SpawnableCollectionId, prefab.PrefabId, createIfMissing: true);

            for (int i = 0; i < count; i++)
            {
                NetworkObject nob = Instantiate(prefab);
                nob.gameObject.SetActive(false);
                cache.Push(nob);
                added.Add(nob);
            }

            return added;
        }

        /// <summary>
        /// Clears pooled objects for a specific NetworkObject.
        /// </summary>
        /// <param name = "nob">Prefab or Instantiated NetworkObject to clear pool for.</param>
        /// <remarks>This will clear the entire pool for the specified object.</remarks>
        public void ClearPool(NetworkObject nob)
        {
            if (!_enabled)
                return;
            if (nob == null)
                return;

            int spawnableCollectionId = nob.SpawnableCollectionId;
            Stack<NetworkObject> stack = GetCache(spawnableCollectionId, nob.PrefabId, createIfMissing: false);
            if (stack == null)
                return;

            DestroyStackNetworkObjectsAndClear(stack);
            _cache[spawnableCollectionId].Clear();
        }

        /// <summary>
        /// Clears all pooled objects.
        /// </summary>
        public void ClearPool()
        {
            int count = _cache.Count;
            for (int i = 0; i < count; i++)
                ClearPool(i);
        }

        /// <summary>
        /// Clears a pool destroying objects for a SpawnableCollectionId.
        /// </summary>
        /// <param name = "spawnableCollectionId">CollectionId to clear for.</param>
        public void ClearPool(int spawnableCollectionId)
        {
            if (spawnableCollectionId >= _cacheCount)
                return;

            Dictionary<int, Stack<NetworkObject>> dict = _cache[spawnableCollectionId];

            foreach (Stack<NetworkObject> item in dict.Values)
                DestroyStackNetworkObjectsAndClear(item);

            dict.Clear();
        }

        /// <summary>
        /// Gets a cache for an id or creates one if does not exist.
        /// </summary>
        /// <returns></returns>
        public Stack<NetworkObject> GetCache(int collectionId, int prefabId, bool createIfMissing)
        {
            if (collectionId >= _cacheCount)
            {
                // Do not create if missing.
                if (!createIfMissing)
                    return null;

                // Add more to the cache.
                while (_cache.Count <= collectionId)
                {
                    Dictionary<int, Stack<NetworkObject>> dict = new();
                    _cache.Add(dict);
                }
                _cacheCount = _cache.Count;
            }

            Dictionary<int, Stack<NetworkObject>> dictionary = _cache[collectionId];
            // No cache for prefabId yet, make one.
            if (!dictionary.TryGetValueIL2CPP(prefabId, out Stack<NetworkObject> cache))
            {
                if (createIfMissing)
                {
                    cache = new();
                    dictionary[prefabId] = cache;
                }
            }

            return cache;
        }

        [Obsolete("Use GetCache(int, int, bool)")]
        public Stack<NetworkObject> GetOrCreateCache(int collectionId, int prefabId) => GetCache(collectionId, prefabId, createIfMissing: true);

        /// <summary>
        /// Destroys all NetworkObjects within a stack and clears the stack.
        /// </summary>
        private void DestroyStackNetworkObjectsAndClear(Stack<NetworkObject> stack)
        {
            foreach (NetworkObject networkObject in stack)
            {
                if (networkObject != null)
                    Destroy(networkObject.gameObject);
            }

            stack.Clear();
        }
    }
}