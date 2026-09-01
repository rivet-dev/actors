using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Settings to apply when loading a scene.
    /// </summary>
    public class LoadOptions
    {
        /// <summary>
        /// True if to automatically unload the loaded scenes when they are no longer being used by clients. This field only applies to scenes loaded for connections, not globally loaded scenes.
        /// </summary>
        [System.NonSerialized]
        public bool AutomaticallyUnload = true;
        /// <summary>
        /// False if to only load scenes which are not yet loaded. When true a scene may load multiple times; this is known as scene stacking. Only the server is able to stack scenes; clients will load a single instance. Global scenes cannot be stacked.
        /// </summary>
        [System.NonSerialized]
        public bool AllowStacking;
        /// <summary>
        /// LocalPhysics mode to use when loading this scene. Generally this will only be used when applying scene stacking. Only used by the server.
        /// https://docs.unity3d.com/ScriptReference/SceneManagement.LocalPhysicsMode.html
        /// </summary>
        [System.NonSerialized]
        public LocalPhysicsMode LocalPhysics = LocalPhysicsMode.None;
        /// <summary>
        /// True to reload a scene if it's already loaded.
        /// This does not function yet.
        /// </summary>
        [System.Obsolete("This feature is not functional yet but will be at a later release.")]
        public bool ReloadScenes;
        /// <summary>
        /// True if scenes should be loaded using addressables. This field only exists for optional use so the user may know if their queue data is using addressables.
        /// </summary>
        public bool Addressables;
    }
}