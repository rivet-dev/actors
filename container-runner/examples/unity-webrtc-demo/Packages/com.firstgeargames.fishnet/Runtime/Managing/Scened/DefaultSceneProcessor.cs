using System.Collections.Generic;
using UnityEngine;
using UnitySceneManager = UnityEngine.SceneManagement.SceneManager;
using UnityScene = UnityEngine.SceneManagement.Scene;
using System.Collections;
using System;

namespace FishNet.Managing.Scened
{
    public class DefaultSceneProcessor : SceneProcessorBase
    {
        #region Private.
        /// <summary>
        /// Currently active loading AsyncOperations.
        /// </summary>
        protected List<AsyncOperation> LoadingAsyncOperations = new();
        /// <summary>
        /// A collection of scenes used both for loading and unloading.
        /// </summary>
        protected List<UnityScene> Scenes = new();
        /// <summary>
        /// Current AsyncOperation being processed.
        /// </summary>
        protected AsyncOperation CurrentAsyncOperation;
        /// <summary>
        /// Last scene to load or begin loading.
        /// </summary>
        private UnityScene _lastLoadedScene;
        #endregion

        /// <summary>
        /// Called when scene loading has begun.
        /// </summary>
        public override void LoadStart(LoadQueueData queueData)
        {
            base.LoadStart(queueData);
            ResetValues();
        }

        public override void LoadEnd(LoadQueueData queueData)
        {
            base.LoadEnd(queueData);
            ResetValues();
        }

        /// <summary>
        /// Resets values for a fresh load or unload.
        /// </summary>
        private void ResetValues()
        {
            CurrentAsyncOperation = null;
            LoadingAsyncOperations.Clear();
        }

        /// <summary>
        /// Called when scene unloading has begun within an unload operation.
        /// </summary>
        /// <param name = "queueData"></param>
        public override void UnloadStart(UnloadQueueData queueData)
        {
            base.UnloadStart(queueData);
            Scenes.Clear();
        }

        /// <summary>
        /// Begin loading a scene using an async method.
        /// </summary>
        /// <param name = "sceneName">Scene name to load.</param>
        public override void BeginLoadAsync(string sceneName, UnityEngine.SceneManagement.LoadSceneParameters parameters)
        {
            AsyncOperation ao = UnitySceneManager.LoadSceneAsync(sceneName, parameters);
            LoadingAsyncOperations.Add(ao);

            _lastLoadedScene = UnitySceneManager.GetSceneAt(UnitySceneManager.sceneCount - 1);

            CurrentAsyncOperation = ao;
            CurrentAsyncOperation.allowSceneActivation = false;
        }

        /// <summary>
        /// Begin unloading a scene using an async method.
        /// </summary>
        /// <param name = "sceneName">Scene name to unload.</param>
        public override void BeginUnloadAsync(UnityScene scene)
        {
            CurrentAsyncOperation = UnitySceneManager.UnloadSceneAsync(scene);
        }

        /// <summary>
        /// Returns if a scene load or unload percent is done.
        /// </summary>
        /// <returns></returns>
        public override bool IsPercentComplete()
        {
            return GetPercentComplete() >= 0.9f;
        }

        /// <summary>
        /// Returns the progress on the current scene load or unload.
        /// </summary>
        /// <returns></returns>
        public override float GetPercentComplete()
        {
            return CurrentAsyncOperation == null ? 1f : CurrentAsyncOperation.progress;
        }

        /// <summary>
        /// Gets the scene last loaded by the processor.
        /// </summary>
        /// <remarks>This is called after IsPercentComplete returns true.</remarks>
        public override UnityScene GetLastLoadedScene() => _lastLoadedScene;

        /// <summary>
        /// Adds a loaded scene.
        /// </summary>
        /// <param name = "scene">Scene loaded.</param>
        public override void AddLoadedScene(UnityScene scene)
        {
            base.AddLoadedScene(scene);
            Scenes.Add(scene);
        }

        /// <summary>
        /// Returns scenes which were loaded during a load operation.
        /// </summary>
        public override List<UnityScene> GetLoadedScenes()
        {
            return Scenes;
        }

        /// <summary>
        /// Activates scenes which were loaded.
        /// </summary>
        public override void ActivateLoadedScenes()
        {
            for (int i = 0; i < LoadingAsyncOperations.Count; i++)
            {
                try
                {
                    LoadingAsyncOperations[i].allowSceneActivation = true;
                }
                catch (Exception e)
                {
                    SceneManager.NetworkManager.LogError($"An error occured while activating scenes. {e.Message}");
                }
            }
        }

        /// <summary>
        /// Returns if all asynchronized tasks are considered IsDone.
        /// </summary>
        /// <returns></returns>
        public override IEnumerator AsyncsIsDone()
        {
            bool notDone;
            do
            {
                notDone = false;
                foreach (AsyncOperation ao in LoadingAsyncOperations)
                {
                    if (!ao.isDone)
                    {
                        notDone = true;
                        break;
                    }
                }
                yield return null;
            } while (notDone);
        }
    }
}