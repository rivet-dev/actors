#if UNITY_EDITOR
using FishNet.Utility;
using System.Collections.Generic;
using UnityEditor;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace FishNet.Editing
{
    public static class Finding
    {
        #region Private.
        /// <summary>
        /// Path where the FishNet.Runtime assembly is.
        /// </summary>
        [System.NonSerialized]
        private static string _fishNetRuntimePath = string.Empty;
        /// <summary>
        /// Path where the FishNet.Generated assembly is.
        /// </summary>
        private static string _fishNetGeneratedPath = string.Empty;
        #endregion

        /// <summary>
        /// Sets FishNet assembly paths.
        /// </summary>
        /// <param name = "error"></param>
        private static void UpdateFishNetPaths()
        {
            if (_fishNetGeneratedPath != string.Empty && _fishNetRuntimePath != string.Empty)
                return;

            string[] guids = AssetDatabase.FindAssets("t:asmdef", new string[] { "Assets" });
            string[] objectPaths = new string[guids.Length];
            for (int i = 0; i < guids.Length; i++)
                objectPaths[i] = AssetDatabase.GUIDToAssetPath(guids[i]);

            string runtimeName = (UtilityConstants.RUNTIME_ASSEMBLY_NAME + ".asmdef").ToLower();
            string generatedName = (UtilityConstants.GENERATED_ASSEMBLY_NAME + ".asmdef").ToLower();
            /* Find all network managers which use Single prefab linking
             * as well all network object prefabs. */
            foreach (string item in objectPaths)
            {
                // Found directory to create object in.
                if (item.ToLower().Contains(runtimeName))
                    _fishNetRuntimePath = System.IO.Path.GetDirectoryName(item);
                else if (item.ToLower().Contains(generatedName))
                    _fishNetGeneratedPath = System.IO.Path.GetDirectoryName(item);

                if (_fishNetGeneratedPath != string.Empty && _fishNetRuntimePath != string.Empty)
                    return;
            }
        }

        /// <summary>
        /// Gets all GameObjects in Assets and optionally scenes.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        public static List<GameObject> GetGameObjects(bool userAssemblies, bool fishNetAssembly, bool includeScenes, string[] ignoredPaths = null)
        {
            List<GameObject> results = new();

            string[] guids;
            string[] objectPaths;

            UpdateFishNetPaths();

            guids = AssetDatabase.FindAssets("t:GameObject", null);
            objectPaths = new string[guids.Length];
            for (int i = 0; i < guids.Length; i++)
                objectPaths[i] = AssetDatabase.GUIDToAssetPath(guids[i]);

            foreach (string item in objectPaths)
            {
                bool inFishNet = item.Contains(_fishNetRuntimePath);
                if (inFishNet && !fishNetAssembly)
                    continue;
                if (!inFishNet && !userAssemblies)
                    continue;
                if (ignoredPaths != null)
                {
                    bool ignore = false;
                    foreach (string path in ignoredPaths)
                    {
                        if (item.Contains(path))
                        {
                            ignore = true;
                            break;
                        }
                    }
                    if (ignore)
                        continue;
                }

                GameObject go = (GameObject)AssetDatabase.LoadAssetAtPath(item, typeof(GameObject));
                results.Add(go);
            }

            if (includeScenes)
                results.AddRange(GetSceneGameObjects());

            return results;
        }

        /// <summary>
        /// Gets all GameObjects in all open scenes.
        /// </summary>
        /// <returns></returns>
        private static List<GameObject> GetSceneGameObjects()
        {
            List<GameObject> results = new();

            for (int i = 0; i < SceneManager.sceneCount; i++)
                results.AddRange(GetSceneGameObjects(SceneManager.GetSceneAt(i)));

            return results;
        }

        /// <summary>
        /// Gets all GameObjects in a scene.
        /// </summary>
        private static List<GameObject> GetSceneGameObjects(Scene s)
        {
            List<GameObject> results = new();
            List<Transform> buffer = new();
            // Iterate all root objects for the scene.
            GameObject[] gos = s.GetRootGameObjects();
            for (int i = 0; i < gos.Length; i++)
            {
                /* Get GameObjects within children of each
                 * root object then add them to the cache. */
                gos[i].GetComponentsInChildren(true, buffer);
                foreach (Transform t in buffer)
                    results.Add(t.gameObject);
            }

            return results;
        }

        /// <summary>
        /// Gets created ScriptableObjects of T.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        public static List<UnityEngine.Object> GetScriptableObjects<T>(bool fishNetAssembly, bool breakOnFirst = false)
        {
            System.Type tType = typeof(T);
            List<UnityEngine.Object> results = new();

            string[] guids = AssetDatabase.FindAssets("t:ScriptableObject", new string[] { "Assets" });
            string[] objectPaths = new string[guids.Length];
            for (int i = 0; i < guids.Length; i++)
                objectPaths[i] = AssetDatabase.GUIDToAssetPath(guids[i]);


            /* This might be faster than using directory comparers.
             * Don't really care since this occurs only at edit. */
            List<string> fishNetPaths = new();
            fishNetPaths.Add(_fishNetGeneratedPath.Replace(@"/", @"\"));
            fishNetPaths.Add(_fishNetGeneratedPath.Replace(@"\", @"/"));
            fishNetPaths.Add(_fishNetRuntimePath.Replace(@"/", @"\"));
            fishNetPaths.Add(_fishNetRuntimePath.Replace(@"\", @"/"));
            /* Find all network managers which use Single prefab linking
             * as well all network object prefabs. */
            foreach (string item in objectPaths)
            {
                // This will skip hidden unity types.
                if (!item.EndsWith(".asset"))
                    continue;
                if (fishNetAssembly)
                {
                    bool found = false;
                    foreach (string path in fishNetPaths)
                    {
                        if (item.Contains(path))
                        {
                            found = true;
                            break;
                        }
                    }
                    if (!found)
                        continue;
                }
                UnityEngine.Object obj = AssetDatabase.LoadAssetAtPath(item, tType);
                if (obj != null && tType != null && obj.GetType() == tType)
                {
                    results.Add(obj);
                    if (breakOnFirst)
                        return results;
                }
            }

            return results;
        }
    }
}
#endif