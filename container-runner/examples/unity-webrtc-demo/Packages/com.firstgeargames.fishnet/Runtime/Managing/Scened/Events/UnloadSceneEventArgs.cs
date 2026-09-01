using System;
using System.Collections.Generic;
using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Data container about a scene unload start.
    /// </summary>
    public struct SceneUnloadStartEventArgs
    {
        /// <summary>
        /// Queue data used by the current scene action.
        /// </summary>
        public readonly UnloadQueueData QueueData;

        internal SceneUnloadStartEventArgs(UnloadQueueData sqd)
        {
            QueueData = sqd;
        }
    }

    /// <summary>
    /// Data container about a scene unload end.
    /// </summary>
    public struct SceneUnloadEndEventArgs
    {
        /// <summary>
        /// Queue data used by the current scene action.
        /// </summary>
        public readonly UnloadQueueData QueueData;
        /// <summary>
        /// Scenes which were successfully unloaded.
        /// This collection may be populated with empty scenes depending on engine version.
        /// </summary>
        [Obsolete("Use UnloadedScenesV2.")] // Remove on V5. Rename UnloadedScenesV2 to UnloadedScenes.
        public List<Scene> UnloadedScenes;
        /// <summary>
        /// Scenes which were successfully unloaded.
        /// This contains information of the scene unloaded but may not contain scene references as some Unity versions discard that information after a scene is unloaded.
        /// </summary>
        public List<UnloadedScene> UnloadedScenesV2;

        internal SceneUnloadEndEventArgs(UnloadQueueData sqd, List<Scene> unloadedScenes, List<UnloadedScene> newUnloadedScenes)
        {
            QueueData = sqd;
#pragma warning disable CS0618 // Type or member is obsolete
            UnloadedScenes = unloadedScenes;
#pragma warning restore CS0618 // Type or member is obsolete
            UnloadedScenesV2 = newUnloadedScenes;
        }
    }
}