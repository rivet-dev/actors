using FishNet.Managing;
using FishNet.Managing.Object;
using FishNet.Object;
using System;
using System.Collections.Generic;
using FishNet.Utility.Extension;
using UnityEngine;

namespace FishNet.Utility.Performance
{
    public abstract class ObjectPool : MonoBehaviour
    {
        /// <summary>
        /// NetworkManager this ObjectPool belongs to.
        /// </summary>
        protected NetworkManager NetworkManager { get; private set; }

        /// <summary>
        /// Called at the end of every frame. This can be used to perform routine tasks.
        /// </summary>
        public virtual void LateUpdate() { }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        public virtual void InitializeOnce(NetworkManager nm)
        {
            NetworkManager = nm;
        }

        /// <summary>
        /// Returns an object that has been stored. A new object will be created if no stored objects are available.
        /// </summary>
        /// <param name = "prefabId">PrefabId of the object to return.</param>
        /// <param name = "collectionId">CollectionId of the object to return.</param>
        /// <param name = "asServer">True if being called on the server side.</param>
        /// <returns></returns>
        [Obsolete("Use RetrieveObject(int, ushort, RetrieveOption, parent, Vector3?, Quaternion? Vector3?, bool) instead.")] // Remove in V5
        public virtual NetworkObject RetrieveObject(int prefabId, ushort collectionId, Transform parent = null, Vector3? position = null, Quaternion? rotation = null, Vector3? scale = null, bool makeActive = true, bool asServer = true) => null;

        /// <summary>
        /// Returns an object that has been stored. A new object will be created if no stored objects are available.
        /// </summary>
        /// <param name = "prefabId">PrefabId of the object to return.</param>
        /// <param name = "collectionId">CollectionId of the object to return.</param>
        /// <param name = "asServer">True if being called on the server side.</param>
        /// <returns></returns>
        public virtual NetworkObject RetrieveObject(int prefabId, ushort collectionId, ObjectPoolRetrieveOption options, Transform parent = null, Vector3? position = null, Quaternion? rotation = null, Vector3? scale = null, bool asServer = true) => null;

        /// <summary>
        /// Returns a prefab using specified values.
        /// </summary>
        /// <param name = "prefabId">PrefabId of the object to return.</param>
        /// <param name = "collectionId">CollectionId of the object to return.</param>
        /// <param name = "asServer">True if being called on the server side.</param>
        /// <returns></returns>
        public virtual NetworkObject GetPrefab(int prefabId, ushort collectionId, bool asServer)
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
        public abstract void StoreObject(NetworkObject instantiated, bool asServer);

        /// <summary>
        /// Instantiates a number of objects and adds them to the pool.
        /// </summary>
        /// <param name = "prefab">Prefab to cache.</param>
        /// <param name = "count">Quantity to spawn.</param>
        /// <param name = "asServer">True if storing prefabs for the server collection. This is only applicable when using DualPrefabObjects.</param>
        public virtual void CacheObjects(NetworkObject prefab, int count, bool asServer) { }

        /// <summary>
        /// Instantiates a number of objects and adds them to the pool.
        /// </summary>
        /// <param name = "prefab">Prefab to cache.</param>
        /// <param name = "count">Quantity to spawn.</param>
        /// <param name = "asServer">True if storing prefabs for the server collection. This is only applicable when using DualPrefabObjects.</param>
        /// <returns>Prefabs instantiated and added to cache.</returns>
        public virtual List<NetworkObject> StorePrefabObjects(NetworkObject prefab, int count, bool asServer) => default;

        /// <summary>
        /// Sets the transform of a NetworkObject to it's values during serialization.
        /// </summary>
        public static void SetTransformToSerializedValues(NetworkObject nob)
        {
            nob.transform.SetLocalPositionRotationAndScale(nob.SerializedTransformProperties.Position, nob.SerializedTransformProperties.Rotation, nob.SerializedTransformProperties.Scale);
        }
    }
}