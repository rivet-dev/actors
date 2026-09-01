// This file contains values serialized in editor or once at runtime.

using UnityEngine;
using System;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using FishNet.Managing;
using FishNet.Utility.Extension;
#if UNITY_EDITOR
using UnityEditor.Experimental.SceneManagement;
using UnityEditor.SceneManagement;
using UnityEditor;
#endif

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// Networked PrefabId assigned to this Prefab.
        /// </summary>
        [field: SerializeField]
        [field: HideInInspector]
        public ushort PrefabId { get; internal set; } = UNSET_PREFABID_VALUE;
        /// <summary>
        /// Spawn collection to use assigned to this Prefab.
        /// </summary>
        [field: SerializeField]
        [field: HideInInspector]
        public ushort SpawnableCollectionId { get; internal set; } = 0;

        /// <summary>
        /// Sets SceneId value. This is not synchronized automatically.
        /// </summary>
        /// <param name = "sceneId"></param>
        public void SetSceneId(ulong sceneId) => SceneId = sceneId;

        /// <summary>
        /// Hash for the path which this asset resides. This value is set during edit time.
        /// </summary>
        [field: SerializeField]
        [field: HideInInspector]
        public ulong AssetPathHash { get; private set; }

        /// <summary>
        /// Sets AssetPathhash value.
        /// </summary>
        /// <param name = "value">Value to use.</param>
        public void SetAssetPathHash(ulong value) => AssetPathHash = value;
        #endregion

        #region Internal.
        /// <summary>
        /// NetworkId for this scene object.
        /// </summary>
        [field: SerializeField]
        [field: HideInInspector]
        internal ulong SceneId;
        /// <summary>
        /// Local properties of the transform during serialization.
        /// </summary>
        [SerializeField]
        [HideInInspector]
        internal TransformProperties SerializedTransformProperties = new();
        #endregion

        #region Private.
        /// <summary>
        /// Last time sceneIds were built automatically.
        /// </summary>
        [NonSerialized]
        private static double _lastSceneIdAutomaticRebuildTime;
        #endregion

        /// <summary>
        /// Removes SceneObject state.
        /// This may only be called at runtime.
        /// </summary>
        internal void ClearRuntimeSceneObject()
        {
            if (!Application.isPlaying)
            {
                NetworkManager.LogError($"ClearRuntimeSceneObject may only be called at runtime.");
                return;
            }

            SceneId = UNSET_SCENEID_VALUE;
        }

        #if UNITY_EDITOR
        private void OnApplicationQuit()
        {
            _lastSceneIdAutomaticRebuildTime = 0;
        }

        /// <summary>
        /// Tries to generate a SceneIds for NetworkObjects in a scene.
        /// </summary>
        internal static List<NetworkObject> CreateSceneId(UnityEngine.SceneManagement.Scene scene, bool force, out int changed)
        {
            changed = 0;

            if (Application.isPlaying)
                return new();
            if (!scene.IsValid())
                return new();
            if (!scene.isLoaded)
                return new();

            HashSet<ulong> setIds = new();
            uint scenePathHash = scene.path.GetStableHashU32();
            List<NetworkObject> sceneNobs = new();

            Scenes.GetSceneNetworkObjects(scene, firstOnly: false, errorOnDuplicates: false, ignoreUnsetSceneIds: false, ref sceneNobs);
            System.Random rnd = new();

            // NetworkObjects which need their Ids rebuilt.
            List<NetworkObject> rebuildingNobs = new();

            foreach (NetworkObject item in sceneNobs)
            {
                bool canGenerate = !item.IsSceneObject || !setIds.Add(item.SceneId);
                /* If an Id has not been generated yet or if it
                 * already exist then rebuild for this object. */
                if (force || canGenerate)
                {
                    item.SceneId = UNSET_SCENEID_VALUE;
                    rebuildingNobs.Add(item);
                }
            }

            foreach (NetworkObject item in rebuildingNobs)
            {
                ulong nextSceneId = UNSET_SCENEID_VALUE;
                while (nextSceneId == UNSET_SCENEID_VALUE || setIds.Contains(nextSceneId))
                {
                    uint rndId = (uint)(rnd.Next(int.MinValue, int.MaxValue) + int.MaxValue);
                    nextSceneId = CombineHashes(scenePathHash, rndId);
                }

                ulong CombineHashes(uint a, uint b)
                {
                    return b | a;
                }

                setIds.Add(nextSceneId);
                changed++;
                item.SceneId = nextSceneId;
                EditorUtility.SetDirty(item);
            }

            return sceneNobs;
        }

        /// <summary>
        /// Tries to generate a SceneId.
        /// </summary>
        private void CreateSceneId(bool force)
        {
            if (Application.isPlaying)
                return;
            // Unity bug, sometimes this can be null depending on editor callback orders.
            if (gameObject == null)
                return;
            // Not a scene object.
            if (string.IsNullOrEmpty(gameObject.scene.name))
            {
                SceneId = UNSET_SCENEID_VALUE;
                return;
            }

            /* If building then only check if
             * scene networkobjects have their sceneIds
             * missing. */
            if (BuildPipeline.isBuildingPlayer)
            {
                // If prefab or part of a prefab, not a scene object.            
                if (PrefabUtility.IsPartOfPrefabAsset(this) || IsEditingInPrefabMode() ||
                    // Not in a scene, another prefab check.
                    !gameObject.scene.IsValid() ||
                    // Stored on disk, so is a prefab. Somehow prefabutility missed it.
                    EditorUtility.IsPersistent(this))
                    // If here this is a sceneObject, but sceneId is not set.
                    if (!IsSceneObject)
                        throw new InvalidOperationException($"Networked GameObject {gameObject.name} in scene {gameObject.scene.path} is missing a SceneId. Use the Fish-Networking menu -> Utility -> Reserialize NetworkObjects > Reserialize Scenes. If the problem persist ensures {gameObject.name} does not have any missing script references on it's prefab or in the scene. Also ensure that you have any prefab changes for the object applied.");
            }
            // If not building check to rebuild sceneIds this for object and the scene its in.
            else
            {
                double realtime = EditorApplication.timeSinceStartup;
                // Only do this once every Xms to prevent excessive rebiulds.
                if (realtime - _lastSceneIdAutomaticRebuildTime < 0.250d)
                    return;

                // Not in a scene, another prefab check.
                // Stored on disk, so is a prefab. Somehow prefabutility missed it.
                if (PrefabUtility.IsPartOfPrefabAsset(this) || IsEditingInPrefabMode() || !gameObject.scene.IsValid() || EditorUtility.IsPersistent(this))
                    return;

                _lastSceneIdAutomaticRebuildTime = realtime;

                CreateSceneId(gameObject.scene, force, out _);
            }
        }

        private bool IsEditingInPrefabMode()
        {
            // if the game object is stored on disk, it is a prefab of some kind, despite not returning true for IsPartOfPrefabAsset =/
            if (EditorUtility.IsPersistent(this))
                return true;

            // If the GameObject is not persistent let's determine which stage we are in first because getting Prefab info depends on it
            StageHandle mainStage = StageUtility.GetMainStageHandle();
            StageHandle currentStage = StageUtility.GetStageHandle(gameObject);
            if (currentStage != mainStage)
            {
                PrefabStage prefabStage = PrefabStageUtility.GetPrefabStage(gameObject);
                if (prefabStage != null)
                    return true;
            }
            
            return false;
        }

        private void ReferenceIds_Reset()
        {
            CreateSceneId(force: false);
        }
        #endif
    }
}