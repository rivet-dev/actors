using FishNet.Managing;
using FishNet.Object;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace FishNet.Utility.Extension
{
    public static class Scenes
    {
        /// <summary>
        /// Gets all NetworkObjects in a scene.
        /// </summary>
        /// <param name = "s">Scene to get objects in.</param>
        /// <param name = "firstOnly">True to only return the first NetworkObject within an object chain. False will return nested NetworkObjects.</param>
        /// <returns></returns>
        public static void GetSceneNetworkObjects(Scene s, bool firstOnly, bool errorOnDuplicates, bool ignoreUnsetSceneIds, ref List<NetworkObject> result)
        {
            List<NetworkObject> nobCacheA = CollectionCaches<NetworkObject>.RetrieveList();
            List<NetworkObject> nobCacheB = CollectionCaches<NetworkObject>.RetrieveList();
            List<GameObject> gameObjectCache = CollectionCaches<GameObject>.RetrieveList();
            Dictionary<ulong, NetworkObject> sceneIds = CollectionCaches<ulong, NetworkObject>.RetrieveDictionary();

            // Iterate all root objects for the scene.
            s.GetRootGameObjects(gameObjectCache);
            foreach (GameObject go in gameObjectCache)
            {
                // Get NetworkObjects within children of each root.
                go.GetComponentsInChildren(true, nobCacheA);
                // If network objects are found.
                if (nobCacheA.Count > 0)
                {
                    // Add only the first networkobject 
                    if (firstOnly)
                    {
                        /* The easiest way to see if a nob is nested is to
                         * get nobs in parent and if the count is greater than 1, then
                         * it is nested. The technique used here isn't exactly fast but
                         * it will only occur during scene loads, so I'm trading off speed
                         * for effort and readability. */
                        foreach (NetworkObject nob in nobCacheA)
                        {
                            if (ignoreUnsetSceneIds && !nob.IsSceneObject)
                                continue;

                            nob.GetComponentsInParent(true, nobCacheB);
                            // No extra nobs, only this one.
                            if (nobCacheB.Count == 1 && !TryDisplayDuplicateError(nob))
                                result.Add(nob);
                        }
                    }
                    // Not first only, add them all.
                    else
                    {
                        foreach (NetworkObject item in nobCacheA)
                        {
                            if (ignoreUnsetSceneIds && !item.IsSceneObject)
                                continue;
                            if (!TryDisplayDuplicateError(item))
                                result.Add(item);
                        }
                    }
                }
            }

            CollectionCaches<ulong, NetworkObject>.Store(sceneIds);

            bool TryDisplayDuplicateError(NetworkObject nob)
            {
                if (!errorOnDuplicates)
                    return false;

                ulong id = nob.SceneId;
                // There is a duplicate.
                if (sceneIds.TryGetValue(id, out NetworkObject originalNob))
                {
                    string err = $"Object {nob.name} and {originalNob.name} in scene {nob.gameObject.scene.name} have the same sceneId of {id}. This will result in spawning errors. Exit play mode and use the Fish-Networking menu to reserialize sceneIds for scene {nob.gameObject.scene.name}.";
                    NetworkManagerExtensions.LogError(err);
                    return true;
                }
                else
                {
                    sceneIds[id] = nob;
                    return false;
                }
            }
        }
    }
}