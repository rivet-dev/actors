#if UNITY_EDITOR
#if MIRROR
using UnityEditor;
using UnityEngine;
using FishNet.Object;
using FishNet.Documenting;
using System.Collections.Generic;
using FNNetworkTransform = FishNet.Component.Transforming.NetworkTransform;
using FNNetworkAnimator = FishNet.Component.Animating.NetworkAnimator;
using FNNetworkObserver = FishNet.Observing.NetworkObserver;
using FishNet.Observing;
using FishNet.Component.Observing;
using FishNet.Editing;
using System.IO;
using System.Collections;
using Mirror;
using MirrorNetworkTransformBase = Mirror.NetworkTransformBase;
using MirrorNetworkTransformChild = Mirror.NetworkTransformChild;
using MirrorNetworkAnimator = Mirror.NetworkAnimator;
#if FGG_ASSETS
using FlexNetworkAnimator = FirstGearGames.Mirrors.Assets.FlexNetworkAnimators.FlexNetworkAnimator;
using FlexNetworkTransformBase = FirstGearGames.Mirrors.Assets.FlexNetworkTransforms.FlexNetworkTransformBase;
using FastProximityChecker = FirstGearGames.Mirrors.Assets.NetworkProximities.FastProximityChecker;
#endif

#if FGG_PROJECTS
using FlexSceneChecker = FirstGearGames.FlexSceneManager.FlexSceneChecker;
#endif

namespace FishNet.Upgrading.Mirror.Editing
{
    /* IMPORTANT IMPORTANT IMPORTANT IMPORTANT 
    * If you receive errors about missing Mirror components,
    * such as NetworkIdentity, then remove MIRROR and any other
    * MIRROR defines.
    * Project Settings -> Player -> Other -> Scripting Define Symbols.
    * 
    * If you are also using my assets add FGG_ASSETS to the defines, and
    * then remove it after running this script. */
    [APIExclude]
    [ExecuteInEditMode]
    [InitializeOnLoad]
    public class MirrorUpgrade : MonoBehaviour
    {
        /// <summary>
        /// SceneCondition within FishNet.
        /// </summary>
        private SceneCondition _sceneCondition = null;
        /// <summary>
        /// DistanceCondition created for the user.
        /// </summary>
        private DistanceCondition _distanceCondition = null;
        /// <summary>
        /// 
        /// </summary>
        private int _replacedNetworkTransforms;
        /// <summary>
        /// 
        /// </summary>
        private int _replacedNetworkAnimators;
        /// <summary>
        /// 
        /// </summary>
        private int _replacedNetworkIdentities;
        /// <summary>
        /// 
        /// </summary>
        private int _replacedSceneCheckers;
        /// <summary>
        /// 
        /// </summary>
        private int _replacedProximityCheckers;
        /// <summary>
        /// True if anything was changed.
        /// </summary>
        private bool _changed;
        /// <summary>
        /// Index in gameObjects to iterate.
        /// </summary>
        private int _goIndex = -1;
        /// <summary>
        /// Found gameObjects to iterate.
        /// </summary>
        private List<GameObject> _gameObjects = new List<GameObject>();
        /// <summary>
        /// True if initialized.
        /// </summary>
        private bool _initialized;
        private const string OBJECT_NAME_PREFIX = "MirrorUpgrade";

        private void Awake()
        {
            gameObject.name = OBJECT_NAME_PREFIX;
            Debug.Log($"{gameObject.name} is working. Please wait until this object is removed from your hierarchy.");
            EditorApplication.update += EditorUpdate;
        }

        private void OnDestroy()
        {
            EditorApplication.update -= EditorUpdate;
        }

        private void EditorUpdate()
        {
            if (!_initialized)
            {
                FindConditions(true);
                _gameObjects = Finding.GetGameObjects(true, false, true, new string[] { "/Mirror/" });
                _goIndex = 0;
                _initialized = true;
            }

            if (_goIndex == -1)
                return;
            if (_goIndex >= _gameObjects.Count)
            {
                gameObject.name = $"{OBJECT_NAME_PREFIX} - 100%";
                Debug.Log($"Switched {_replacedNetworkTransforms} NetworkTransforms.");
                Debug.Log($"Switched {_replacedNetworkAnimators} NetworkAnimators.");
                Debug.Log($"Switched {_replacedSceneCheckers} SceneCheckers.");
                Debug.Log($"Switched {_replacedProximityCheckers} ProximityCheckers.");
                Debug.Log($"Switched {_replacedNetworkIdentities} NetworkIdentities.");

                if (_changed)
                    PrintSaveWarning();

                DestroyImmediate(gameObject);
                return;
            }

            float percentFloat = ((float)_goIndex / (float)_gameObjects.Count) * 100f;
            int percentInt = Mathf.FloorToInt(percentFloat);
            gameObject.name = $"{OBJECT_NAME_PREFIX} - {percentInt}%";

            GameObject go = _gameObjects[_goIndex];
            _goIndex++;
            // Go went empty?
            if (go == null)
                return;

            /* When a component is removed
             * changed is set true and remove count is increased.
             * _goIndex is also returned before exiting the method.
             * This will cause the same gameObject to iterate
             * next update. This is important because the components
             * must be Switched in order, and I can only remove one
             * component per frame without Unity throwing a fit and
             * freezing. A while loop doesn't let Unity recognize the component
             * is gone(weird right? maybe editor thing), and a coroutine
             * doesn't show errors well, they just fail silently. */

            bool changedThisFrame = false;
            if (IterateNetworkTransform(go))
            {
                changedThisFrame = true;
                _changed = true;
                _replacedNetworkTransforms++;
            }
            if (IterateNetworkAnimator(go))
            {
                changedThisFrame = true;
                _changed = true;
                _replacedNetworkAnimators++;
            }

            if (IterateSceneChecker(go))
            {
                changedThisFrame = true;
                _changed = true;
                _replacedSceneCheckers++;
            }
            if (IterateProximityChecker(go))
            {
                changedThisFrame = true;
                _changed = true;
                _replacedProximityCheckers++;
            }
            if (changedThisFrame)
            {
                _goIndex--;
                return;
            }
            // NetworkIdentity must be done last.
            if (IterateNetworkIdentity(go))
            {
                _changed = true;
                _replacedNetworkIdentities++;
            }
        }

        /// <summary>
        /// Finds Condition scripts to be used with NetworkObserver.
        /// </summary>
        /// <param name="error"></param>
        private void FindConditions(bool error)
        {
            List<UnityEngine.Object> scriptableObjects;

            if (_sceneCondition == null)
            {
                scriptableObjects = Finding.GetScriptableObjects<SceneCondition>(true, true);
                // Use the first found scene condition, there should be only one.
                if (scriptableObjects.Count > 0)
                    _sceneCondition = (SceneCondition)scriptableObjects[0];

                if (_sceneCondition == null && error)
                    Debug.LogError("SceneCondition could not be found. Upgrading scene checker components will not function.");
            }

            if (_distanceCondition == null)
            {
                scriptableObjects = Finding.GetScriptableObjects<DistanceCondition>(false, true);
                if (scriptableObjects.Count > 0)
                {
                    _distanceCondition = (DistanceCondition)scriptableObjects[0];
                }
                else
                {
                    DistanceCondition dc = ScriptableObject.CreateInstance<DistanceCondition>();
                    string savePath = "Assets";
                    AssetDatabase.CreateAsset(dc, Path.Combine(savePath, $"CreatedDistanceCondition.asset"));
                    Debug.LogWarning($"DistanceCondition has been created at {savePath}. Place this file somewhere within your project and change settings to your liking.");
                }

                if (_distanceCondition == null && error)
                    Debug.LogError("DistanceCondition could not be found. Upgrading proximity checker components will not function.");
            }
        }

        private bool IterateNetworkTransform(GameObject go)
        {
            if (go.TryGetComponent(out MirrorNetworkTransformBase nt1))
            {
                Transform target;
                if (nt1 is MirrorNetworkTransformChild mc1)
                    target = mc1.target;
                else
                    target = go.transform;
                Replace(nt1, target);
                return true;
            }
#if FGG_ASSETS
            if (go.TryGetComponent(out FlexNetworkTransformBase fntb))
            {
                Replace(fntb, fntb.TargetTransform);
                return true;
            }
#endif

            void Replace(UnityEngine.Component component, Transform target)
            {
                EditorUtility.SetDirty(go);
                DestroyImmediate(component, true);

                if (target != null && !target.TryGetComponent<FNNetworkTransform>(out _))
                    target.gameObject.AddComponent<FNNetworkTransform>();
            }

            // Fall through, nothing was replaced.
            return false;
        }

        private bool IterateNetworkAnimator(GameObject go)
        {
            if (go.TryGetComponent(out MirrorNetworkAnimator mna))
            {
                Replace(mna, mna.transform);
                return true;
            }
#if FGG_ASSETS
            if (go.TryGetComponent(out FlexNetworkAnimator fna))
            {
                Replace(fna, fna.transform);
                return true;
            }
#endif

            void Replace(UnityEngine.Component component, Transform target)
            {
                EditorUtility.SetDirty(go);
                DestroyImmediate(component, true);

                if (target == null)
                    return;
                if (!target.TryGetComponent<FNNetworkAnimator>(out _))
                    target.gameObject.AddComponent<FNNetworkAnimator>();
            }

            return false;
        }

        private bool IterateSceneChecker(GameObject go)
        {
            if (_sceneCondition == null)
                return false;

#if FGG_PROJECTS
            if (go.TryGetComponent(out FlexSceneChecker fsc))
            {
                Replace(fsc);
                return true;
            }
#endif

            void Replace(UnityEngine.Component component)
            {
                EditorUtility.SetDirty(go);
                DestroyImmediate(component, true);

                FNNetworkObserver networkObserver;
                if (!go.TryGetComponent(out networkObserver))
                    networkObserver = go.AddComponent<FNNetworkObserver>();

                bool conditionFound = false;
                foreach (ObserverCondition condition in networkObserver.ObserverConditions)
                {
                    if (condition.GetType() == typeof(SceneCondition))
                    {
                        conditionFound = true;
                        break;
                    }
                }

                // If not able to find scene condition then add one.
                if (!conditionFound)
                    networkObserver.ObserverConditionsInternal.Add(_sceneCondition);
            }

            return false;
        }

        private bool IterateProximityChecker(GameObject go)
        {
            if (_distanceCondition == null)
                return false;

#if FGG_PROJECTS
            if (go.TryGetComponent(out FastProximityChecker fpc))
            {
                Replace(fpc);
                return true;
            }
#endif

            void Replace(UnityEngine.Component component)
            {
                EditorUtility.SetDirty(go);
                DestroyImmediate(component, true);

                FNNetworkObserver networkObserver;
                if (!go.TryGetComponent(out networkObserver))
                    networkObserver = go.AddComponent<FNNetworkObserver>();

                bool conditionFound = false;
                foreach (ObserverCondition condition in networkObserver.ObserverConditions)
                {
                    if (condition.GetType() == typeof(DistanceCondition))
                    {
                        conditionFound = true;
                        break;
                    }
                }

                // If not able to find scene condition then add one.
                if (!conditionFound)
                    networkObserver.ObserverConditionsInternal.Add(_distanceCondition);
            }

            return false;
        }

        private bool IterateNetworkIdentity(GameObject go)
        {
            if (go.TryGetComponent(out NetworkIdentity netIdentity))
            {
                EditorUtility.SetDirty(go);
                DestroyImmediate(netIdentity, true);

                // Add nob if doesn't exist.
                if (!go.TryGetComponent<NetworkObject>(out _))
                    go.AddComponent<NetworkObject>();

                return true;
            }

            return false;
        }

        private static void PrintSaveWarning()
        {
            Debug.LogWarning("You must File -> Save for changes to complete.");
        }
    }
}
#endif
#endif