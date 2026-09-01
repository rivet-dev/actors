// using FishNet.Managing.Scened.Data;
// using System;
// using UnityEngine;
// using UnityEngine.SceneManagement;

// namespace FishNet.Managing.Scened
// {

//    public static class SceneSpawner
//    {

//        #region Prefab.
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(Scene scene, GameObject prefab)
//        {
//            return Instantiate<GameObject>(scene, prefab);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(Scene scene, GameObject prefab)
//        {
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, null, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(SceneReferenceData sceneReferenceData, GameObject prefab)
//        {
//            return Instantiate<GameObject>(sceneReferenceData, prefab);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(SceneReferenceData sceneReferenceData, GameObject prefab)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneReferenceData);
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, null, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(int sceneHandle, GameObject prefab)
//        {
//            return Instantiate<GameObject>(sceneHandle, prefab, prefab.transform.position, prefab.transform.rotation, null);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(int sceneHandle, GameObject prefab)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneHandle);
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, null, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(string sceneName, GameObject prefab)
//        {
//            return Instantiate<GameObject>(sceneName, prefab);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(string sceneName, GameObject prefab)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneName);
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, null, true);
//        }
//        #endregion

//        #region Prefab, Parent, WorldSpace
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(Scene scene, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            return Instantiate<GameObject>(scene, prefab, parent, instantiateInWorldSpace);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(Scene scene, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, parent, instantiateInWorldSpace);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(SceneReferenceData sceneReferenceData, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            return Instantiate<GameObject>(sceneReferenceData, prefab, parent, instantiateInWorldSpace);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(SceneReferenceData sceneReferenceData, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneReferenceData);
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, parent, instantiateInWorldSpace);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(int sceneHandle, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            return Instantiate<GameObject>(sceneHandle, prefab, parent, instantiateInWorldSpace);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(int sceneHandle, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneHandle);
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, parent, instantiateInWorldSpace);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(string sceneName, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            return Instantiate<GameObject>(sceneName, prefab, parent, instantiateInWorldSpace);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(string sceneName, GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneName);
//            return Instantiate<T>(scene, prefab, prefab.transform.position, prefab.transform.rotation, parent, instantiateInWorldSpace);
//        }
//        #endregion

//        #region Prefab, Position, Rotation.
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(Scene scene, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            return Instantiate<GameObject>(scene, prefab, position, rotation);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(Scene scene, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            return Instantiate<T>(scene, prefab, position, rotation, null, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(SceneReferenceData sceneReferenceData, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            return Instantiate<GameObject>(sceneReferenceData, prefab, position, rotation);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(SceneReferenceData sceneReferenceData, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneReferenceData);
//            return Instantiate<T>(scene, prefab, position, rotation, null, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(int sceneHandle, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            return Instantiate<GameObject>(sceneHandle, prefab, position, rotation);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(int sceneHandle, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneHandle);
//            return Instantiate<T>(scene, prefab, position, rotation, null, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(string sceneName, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            return Instantiate<GameObject>(sceneName, prefab, position, rotation);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(string sceneName, GameObject prefab, Vector3 position, Quaternion rotation)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneName);
//            return Instantiate<T>(scene, prefab, position, rotation, null, true);
//        }
//        #endregion

//        #region Prefab, Position, Rotation, Parent.
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns> 
//        public static GameObject Instantiate(Scene scene, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            return Instantiate<GameObject>(scene, prefab, position, rotation, parent);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(Scene scene, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            return Instantiate<T>(scene, prefab, position, rotation, parent, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(SceneReferenceData sceneReferenceData, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            return Instantiate<GameObject>(sceneReferenceData, prefab, position, rotation, parent);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(SceneReferenceData sceneReferenceData, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneReferenceData);
//            return Instantiate<T>(scene, prefab, position, rotation, parent, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(int sceneHandle, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            return Instantiate<GameObject>(sceneHandle, prefab, position, rotation, parent);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(int sceneHandle, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneHandle);
//            return Instantiate<T>(scene, prefab, position, rotation, parent, true);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static GameObject Instantiate(string sceneName, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            return Instantiate<GameObject>(sceneName, prefab, position, rotation, parent);
//        }
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        public static T Instantiate<T>(string sceneName, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
//        {
//            Scene scene = SceneManager.ReturnScene(sceneName);
//            return Instantiate<T>(scene, prefab, position, rotation, parent, true);
//        }
//        #endregion

//        #region Instantiator.
//        /// <summary>
//        /// Instantiates a prefab and moves it to a scene.
//        /// </summary>
//        /// <returns>Instantiated prefab or script.</returns>
//        private static T Instantiate<T>(Scene scene, GameObject prefab, Vector3 position, Quaternion rotation, Transform parent, bool instantiateInWorldSpace)
//        {
//            if (string.IsNullOrEmpty(scene.name))
//            {
//                Debug.LogWarning("Scene does not exist. Prefab cannot be instantiated.");
//                return default(T);
//            }

//            GameObject result = MonoBehaviour.Instantiate(prefab, position, rotation);
//            if (result != null)
//            {
//                // Move to new scene first.
//                UnityEngine.SceneManagement.SceneManager.MoveGameObjectToScene(result, scene);

//                // Set parent and spaces.
//                if (parent != null)
//                {
//                    result.transform.SetParent(parent);
//                    // If to not instantiate in world space then update pos/rot to localspace.
//                    if (!instantiateInWorldSpace)
//                    {
//                        result.transform.localPosition = position;
//                        result.transform.localRotation = rotation;
//                    }
//                }

//                // If was a gameobject then return as GO.
//                if (typeof(T) == typeof(GameObject))
//                    return (T)Convert.ChangeType(result, typeof(GameObject));
//                // Otherwise use getcomponent on the type.
//                else
//                    return result.GetComponent<T>();
//            }
//            // Couldn't be instantiated, return default of T.
//            else
//            {
//                return default(T);
//            }

//        }
//        #endregion

//    }

// }

