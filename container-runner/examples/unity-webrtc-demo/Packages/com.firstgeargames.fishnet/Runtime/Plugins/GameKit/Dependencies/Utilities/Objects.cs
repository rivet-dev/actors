using GameKit.Dependencies.Utilities.Types;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace GameKit.Dependencies.Utilities
{
    public static class Objects
    {
        /// <summary>
        /// Returns if an object has been destroyed from memory.
        /// </summary>
        /// <param name = "gameObject"></param>
        /// <returns></returns>
        public static bool IsDestroyed(this GameObject gameObject)
        {
            // UnityEngine overloads the == operator for the GameObject type
            // and returns null when the object has been destroyed, but 
            // actually the object is still there but has not been cleaned up yet
            // if we test both we can determine if the object has been destroyed.
            return gameObject == null && !ReferenceEquals(gameObject, null);
        }

        /// <summary>
        /// Finds all objects in the scene of type. This method is very expensive.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <param name = "requireSceneLoaded">True if the scene must be fully loaded before trying to seek objects.</param>
        /// <returns></returns>
        public static List<T> FindAllObjectsOfType<T>(bool activeSceneOnly = true, bool requireSceneLoaded = false, bool includeDDOL = true, bool includeInactive = true)
        {
            List<T> results = new();
            for (int i = 0; i < SceneManager.sceneCount; i++)
            {
                Scene scene = SceneManager.GetSceneAt(i);
                // If to include only current scene.
                if (activeSceneOnly)
                {
                    if (SceneManager.GetActiveScene() != scene)
                        continue;
                }
                // If the scene must be fully loaded to seek objects within.
                if (!scene.isLoaded && requireSceneLoaded)
                    continue;

                GameObject[] allGameObjects = scene.GetRootGameObjects();
                for (int j = 0; j < allGameObjects.Length; j++)
                {
                    results.AddRange(allGameObjects[j].GetComponentsInChildren<T>(includeInactive));
                }
            }

            // If to also include DDOL.
            if (includeDDOL)
            {
                GameObject ddolGo = DDOL.GetDDOL().gameObject;
                results.AddRange(ddolGo.GetComponentsInChildren<T>(includeInactive));
            }

            return results;
        }
    }
}