using FishNet.Documenting;
using FishNet.Object;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Managing.Object
{
    // document
    [APIExclude]
    public abstract class PrefabObjects : ScriptableObject
    {
        /// <summary>
        /// CollectionId for this PrefabObjects.
        /// </summary>
        public ushort CollectionId { get; private set; }

        /// <summary>
        /// Sets CollectionIdValue.
        /// </summary>
        internal void SetCollectionId(ushort id) => CollectionId = id;

        public abstract void Clear();
        public abstract int GetObjectCount();
        public abstract NetworkObject GetObject(bool asServer, int id);
        public abstract void RemoveNull();
        public abstract void AddObject(NetworkObject networkObject, bool checkForDuplicates = false, bool initializeAdded = true);
        public abstract void AddObjects(List<NetworkObject> networkObjects, bool checkForDuplicates = false, bool initializeAdded = true);
        public abstract void AddObjects(NetworkObject[] networkObjects, bool checkForDuplicates = false, bool initializeAdded = true);
        public abstract void AddObject(DualPrefab dualPrefab, bool checkForDuplicates = false, bool initializeAdded = true);
        public abstract void AddObjects(List<DualPrefab> dualPrefab, bool checkForDuplicates = false, bool initializeAdded = true);
        public abstract void AddObjects(DualPrefab[] dualPrefab, bool checkForDuplicates = false, bool initializeAdded = true);
        public abstract void InitializePrefabRange(int startIndex);
    }
}