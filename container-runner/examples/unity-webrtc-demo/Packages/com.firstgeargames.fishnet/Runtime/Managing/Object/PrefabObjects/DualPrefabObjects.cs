using FishNet.Documenting;
using FishNet.Object;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Managing.Object
{
    // document
    [APIExclude]
    [CreateAssetMenu(fileName = "New DualPrefabObjects", menuName = "FishNet/Spawnable Prefabs/Dual Prefab Objects")]
    public class DualPrefabObjects : PrefabObjects
    {
        /// <summary>
        /// </summary>
        [Tooltip("Prefabs which may be spawned.")]
        [SerializeField]
        private List<DualPrefab> _prefabs = new();
        /// <summary>
        /// Prefabs which may be spawned.
        /// </summary>
        public IReadOnlyList<DualPrefab> Prefabs => _prefabs;

        public override void Clear()
        {
            _prefabs.Clear();
        }

        public override int GetObjectCount()
        {
            return _prefabs.Count;
        }

        public override NetworkObject GetObject(bool asServer, int id)
        {
            if (id < 0 || id >= _prefabs.Count)
            {
                NetworkManagerExtensions.LogError($"PrefabId {id} is out of range.");
                return null;
            }
            else
            {
                DualPrefab dp = _prefabs[id];
                NetworkObject nob = asServer ? dp.Server : dp.Client;
                if (nob == null)
                {
                    string lookupSide = asServer ? "server" : "client";
                    NetworkManagerExtensions.LogError($"Prefab for {lookupSide} on id {id} is null ");
                }

                return nob;
            }
        }

        public override void RemoveNull()
        {
            for (int i = 0; i < _prefabs.Count; i++)
            {
                if (_prefabs[i].Server == null || _prefabs[i].Client == null)
                {
                    _prefabs.RemoveAt(i);
                    i--;
                }
            }
        }

        public override void AddObject(DualPrefab dualPrefab, bool checkForDuplicates = false, bool initializeAdded = true)
        {
            AddObjects(new DualPrefab[] { dualPrefab }, checkForDuplicates, initializeAdded);
        }

        public override void AddObjects(List<DualPrefab> dualPrefabs, bool checkForDuplicates = false, bool initializeAdded = true)
        {
            AddObjects(dualPrefabs.ToArray(), checkForDuplicates, initializeAdded);
        }

        public override void AddObjects(DualPrefab[] dualPrefabs, bool checkForDuplicates = false, bool initializeAdded = true)
        {
            if (!checkForDuplicates)
            {
                _prefabs.AddRange(dualPrefabs);
            }
            else
            {
                foreach (DualPrefab dp in dualPrefabs)
                    AddUniqueNetworkObjects(dp);
            }

            if (initializeAdded && Application.isPlaying)
                InitializePrefabRange(0);
        }

        private void AddUniqueNetworkObjects(DualPrefab dp)
        {
            for (int i = 0; i < _prefabs.Count; i++)
            {
                if (_prefabs[i].Server == dp.Server && _prefabs[i].Client == dp.Client)
                    return;
            }

            _prefabs.Add(dp);
        }

        public override void InitializePrefabRange(int startIndex)
        {
            for (int i = startIndex; i < _prefabs.Count; i++)
            {
                ManagedObjects.InitializePrefab(_prefabs[i].Server, i, CollectionId);
                ManagedObjects.InitializePrefab(_prefabs[i].Client, i, CollectionId);
            }
        }

        #region Unused.
        public override void AddObject(NetworkObject networkObject, bool checkForDuplicates = false, bool initializeAdded = true)
        {
            NetworkManagerExtensions.LogError($"Single prefabs are not supported with DualPrefabObjects. Make a SinglePrefabObjects asset instead.");
        }

        public override void AddObjects(List<NetworkObject> networkObjects, bool checkForDuplicates = false, bool initializeAdded = true)
        {
            NetworkManagerExtensions.LogError($"Single prefabs are not supported with DualPrefabObjects. Make a SinglePrefabObjects asset instead.");
        }

        public override void AddObjects(NetworkObject[] networkObjects, bool checkForDuplicates = false, bool initializeAdded = true)
        {
            NetworkManagerExtensions.LogError($"Single prefabs are not supported with DualPrefabObjects. Make a SinglePrefabObjects asset instead.");
        }
        #endregion
    }
}