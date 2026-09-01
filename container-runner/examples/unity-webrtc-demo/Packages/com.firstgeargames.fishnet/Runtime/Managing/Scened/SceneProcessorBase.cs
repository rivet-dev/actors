using System.Collections;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.SceneManagement;
using UnityScene = UnityEngine.SceneManagement.Scene;
using UnitySceneManager = UnityEngine.SceneManagement.SceneManager;

namespace FishNet.Managing.Scened
{
    public abstract class SceneProcessorBase : MonoBehaviour
    {
        #region Protected.
        /// <summary>
        /// SceneManager for this processor.
        /// </summary>
        protected SceneManager SceneManager;
        /// <summary>
        /// Scene used to store objects while they are being moved from one scene to another.
        /// </summary>
        protected Scene MovedObjectsScene;
        /// <summary>
        /// Scene used to store objects queued for destruction but cannot be destroyed until the clientHost gets the despawn packet.
        /// </summary>
        protected Scene DelayedDestroyScene;
        /// <summary>
        /// Scene used as the active scene when the user does not specify which scene to set active and the scenemanager cannot determine one without error.
        /// This is primarily used so scenes with incorrect or unexpected lighting are not set as the active scene given this may disrupt visuals.
        /// </summary>
        protected Scene FallbackActiveScene;
        #endregion

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        /// <param name = "manager">SceneManager which will be utilizing this class.</param>
        public virtual void Initialize(SceneManager manager)
        {
            SceneManager = manager;
        }

        /// <summary>
        /// Called when scene loading has begun.
        /// </summary>
        public virtual void LoadStart(LoadQueueData queueData) { }

        /// <summary>
        /// Called when scene loading has ended.
        /// </summary>
        public virtual void LoadEnd(LoadQueueData queueData) { }

        /// <summary>
        /// Called when scene unloading has begun within a load operation.
        /// </summary>
        public virtual void UnloadStart(LoadQueueData queueData) { }

        /// <summary>
        /// Called when scene unloading has ended within a load operation.
        /// </summary>
        public virtual void UnloadEnd(LoadQueueData queueData) { }

        /// <summary>
        /// Called when scene unloading has begun within an unload operation.
        /// </summary>
        public virtual void UnloadStart(UnloadQueueData queueData) { }

        /// <summary>
        /// Called when scene unloading has ended within an unload operation.
        /// </summary>
        public virtual void UnloadEnd(UnloadQueueData queueData) { }

        /// <summary>
        /// Begin loading a scene using an async method.
        /// </summary>
        /// <param name = "sceneName">Scene name to load.</param>
        public abstract void BeginLoadAsync(string sceneName, LoadSceneParameters parameters);

        /// <summary>
        /// Begin unloading a scene using an async method.
        /// </summary>
        /// <param name = "sceneName">Scene name to unload.</param>
        public abstract void BeginUnloadAsync(Scene scene);

        /// <summary>
        /// Returns if a scene load or unload percent is done.
        /// </summary>
        /// <returns></returns>
        public abstract bool IsPercentComplete();

        /// <summary>
        /// Returns the progress on the current scene load or unload.
        /// </summary>
        /// <returns></returns>
        public abstract float GetPercentComplete();

        /// <summary>
        /// Gets the scene last loaded by the processor.
        /// </summary>
        /// <remarks>This is called after IsPercentComplete returns true.</remarks>
        public virtual Scene GetLastLoadedScene() => default;

        /// <summary>
        /// Adds a scene to loaded scenes.
        /// </summary>
        /// <param name = "scene">Scene loaded.</param>
        public virtual void AddLoadedScene(Scene scene) { }

        /// <summary>
        /// Returns scenes which were loaded during a load operation.
        /// </summary>
        public abstract List<Scene> GetLoadedScenes();

        /// <summary>
        /// Activates scenes which were loaded.
        /// </summary>
        public abstract void ActivateLoadedScenes();

        /// <summary>
        /// Returns if all asynchronized tasks are considered IsDone.
        /// </summary>
        /// <returns></returns>
        public abstract IEnumerator AsyncsIsDone();

        /// <summary>
        /// Returns the MovedObjectsScene.
        /// </summary>
        /// <returns></returns>
        public virtual Scene GetMovedObjectsScene()
        {
            // Create moved objects scene. It will probably be used eventually. If not, no harm either way.
            if (string.IsNullOrEmpty(MovedObjectsScene.name))
                MovedObjectsScene = FindOrCreateScene("MovedObjectsHolder");

            return MovedObjectsScene;
        }

        /// <summary>
        /// Returns the DelayedDestroyScene.
        /// </summary>
        /// <returns></returns>
        public virtual Scene GetDelayedDestroyScene()
        {
            // Create moved objects scene. It will probably be used eventually. If not, no harm either way.
            if (string.IsNullOrEmpty(DelayedDestroyScene.name))
                DelayedDestroyScene = FindOrCreateScene("DelayedDestroy");

            return DelayedDestroyScene;
        }

        /// <summary>
        /// Returns the FallbackActiveScene.
        /// </summary>
        /// <returns></returns>
        public virtual Scene GetFallbackActiveScene()
        {
            if (string.IsNullOrEmpty(FallbackActiveScene.name))
                FallbackActiveScene = FindOrCreateScene("FallbackActiveScene");

            return FallbackActiveScene;
        }

        /// <summary>
        /// Tries to find a scene by name and if it does not exist creates an empty scene of name.
        /// </summary>
        /// <param name = "name">Name of the scene to find or create.</param>
        /// <returns></returns>
        public virtual Scene FindOrCreateScene(string name)
        {
            Scene result = UnitySceneManager.GetSceneByName(name);
            if (!result.IsValid())
                result = UnitySceneManager.CreateScene(name);

            return result;
        }
    }
}