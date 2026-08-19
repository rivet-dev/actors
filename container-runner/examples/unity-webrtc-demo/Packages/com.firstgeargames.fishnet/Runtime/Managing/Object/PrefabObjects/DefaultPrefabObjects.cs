using FishNet.Documenting;
using System.Collections.Generic;
using UnityEngine;
using GameKit.Dependencies.Utilities;
using System.Text;
#if UNITY_EDITOR
using UnityEditor;
#endif
using FishNet.Object;

namespace FishNet.Managing.Object
{
    [APIExclude]
    // [CreateAssetMenu(fileName = "New DefaultPrefabObjects", menuName = "FishNet/Spawnable Prefabs/Default Prefab Objects")]
    public class DefaultPrefabObjects : SinglePrefabObjects
    {
        /// <summary>
        /// Used for version rebuilding.
        /// </summary>
        private StringBuilder _stringBuilder = new();


        /// <summary>
        /// Sets asset path hashes for prefabs starting at index, or if missing.
        /// </summary
        /// <return>Returns true if one or more NetworkObjects were updated.</return>
        internal bool SetAssetPathHashes(int index)
        {
            #if UNITY_EDITOR
            bool dirtied = false;
            int count = base.GetObjectCount();

            if (count == 0)
                return false;
            
            if (index < 0 || index >= count)
            {
                Debug.LogError($"Index {index} is out of range when trying to set asset path hashes. Collection length is {count}. Defaulf prefabs may need to be rebuilt.");
                return false;
            }

            for (int i = 0; i < count; i++)
            {
                NetworkObject n = Prefabs[i];
                if (i < index)
                    continue;

                string pathAndName = $"{AssetDatabase.GetAssetPath(n.gameObject)}{n.gameObject.name}".Trim().ToLowerInvariant();

                _stringBuilder.Clear();
                foreach (char c in pathAndName)
                {
                    if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9'))
                        _stringBuilder.Append(c);
                }
                
                ulong hashcode = _stringBuilder.ToString().GetStableHashU64();
                // Already set.
                if (n.AssetPathHash == hashcode)
                    continue;

                n.SetAssetPathHash(hashcode);
                EditorUtility.SetDirty(n);
                dirtied = true;
            }

            //Check for conflicts.
            Dictionary<ulong, string> hashesAndPaths = new();
            for (int i = 0; i < count; i++)
            {
                NetworkObject n = Prefabs[i];

                string pathAndName = $"{AssetDatabase.GetAssetPath(n.gameObject)}{n.gameObject.name}";
                
                if (hashesAndPaths.TryGetValueIL2CPP(n.AssetPathHash, out string path)) 
                {
                    Debug.LogError($"Assets {pathAndName} and {path} have the same assetPath hash of {n.AssetPathHash}. Please modify the prefab name of either to resolve.");
                    dirtied = false;
                }
                else
                {
                    hashesAndPaths.Add(n.AssetPathHash, pathAndName);
                }
            }
            
            return dirtied;
            #else
            return false;
            #endif
        }

        /// <summary>
        /// Sorts prefabs by name and path hashcode.
        /// </summary>
        internal void Sort()
        {
            if (base.GetObjectCount() == 0)
                return;

            Dictionary<ulong, NetworkObject> hashcodesAndNobs = new();
            List<ulong> hashcodes = new();

            bool error = false;
            foreach (NetworkObject n in Prefabs)
            {
                hashcodes.Add(n.AssetPathHash);
                // If hashcode is 0 something is wrong
                if (n.AssetPathHash == 0)
                {
                    error = true;
                    Debug.LogError($"AssetPathHash is not set for GameObject {n.name}.");
                }
                hashcodesAndNobs.Add(n.AssetPathHash, n);
            }
            // An error occured, no reason to continue.
            if (error)
            {
                Debug.LogError($"One or more NetworkObject prefabs did not have their AssetPathHash set. This usually occurs when a prefab cannot be saved. Check the specified prefabs for missing scripts or serialization errors and correct them, then use Fish-Networking -> Refresh Default Prefabs.");
                return;
            }

            // Once all hashes have been made re-add them to prefabs sorted.
            hashcodes.Sort();
            // Build to a new list using sorted hashcodes.
            List<NetworkObject> sortedNobs = new();
            foreach (ulong hc in hashcodes)
                sortedNobs.Add(hashcodesAndNobs[hc]);

            base.Clear();
            base.AddObjects(sortedNobs, checkForDuplicates: false, initializeAdded: false);
        }
    }
}