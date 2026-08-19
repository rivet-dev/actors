using System.Collections.Generic;
using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Data container about a scene load start.
    /// </summary>
    public struct SceneLoadStartEventArgs
    {
        /// <summary>
        /// Queue data used by the current scene action.
        /// </summary>
        public readonly LoadQueueData QueueData;

        internal SceneLoadStartEventArgs(LoadQueueData lqd)
        {
            QueueData = lqd;
        }
    }

    /// <summary>
    /// Data container about a scene load percent change.
    /// </summary>
    public struct SceneLoadPercentEventArgs
    {
        /// <summary>
        /// Queue data used by the current scene action.
        /// </summary>
        public readonly LoadQueueData QueueData;
        /// <summary>
        /// Percentage of change completion. 1f is equal to 100% complete.
        /// </summary>
        public readonly float Percent;

        internal SceneLoadPercentEventArgs(LoadQueueData lqd, float percent)
        {
            QueueData = lqd;
            Percent = percent;
        }
    }

    /// <summary>
    /// Data container about a scene load end.
    /// </summary>
    public struct SceneLoadEndEventArgs
    {
        /// <summary>
        /// Queue data used by the current scene action.
        /// </summary>
        public readonly LoadQueueData QueueData;
        /// <summary>
        /// Scenes which were loaded.
        /// </summary>
        public readonly Scene[] LoadedScenes;
        /// <summary>
        /// Scenes which were skipped because they were already loaded.
        /// </summary>
        public readonly string[] SkippedSceneNames;
        /// <summary>
        /// Scenes which were unloaded.
        /// </summary>
        public readonly string[] UnloadedSceneNames;

        internal SceneLoadEndEventArgs(LoadQueueData lqd, string[] skipped, Scene[] loaded, string[] unloadedSceneNames)
        {
            QueueData = lqd;
            SkippedSceneNames = skipped;
            LoadedScenes = loaded;
            UnloadedSceneNames = unloadedSceneNames;
        }
    }
}