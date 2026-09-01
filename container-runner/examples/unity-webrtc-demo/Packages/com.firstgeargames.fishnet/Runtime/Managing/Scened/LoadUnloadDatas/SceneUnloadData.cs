using System.Collections.Generic;
using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Data about which scenes to unload.
    /// </summary>
    public class SceneUnloadData
    {
        /// <summary>
        /// When specified these scenes will be set as the active scene after loading occurs.
        /// </summary>
        public PreferredScene PreferredActiveScene;
        /// <summary>
        /// SceneLookupData for each scene to load.
        /// </summary>
        public SceneLookupData[] SceneLookupDatas = new SceneLookupData[0];
        /// <summary>
        /// Parameters which may be set and will be included in load callbacks.
        /// </summary>
        public UnloadParams Params = new();
        /// <summary>
        /// Additional options to use for loaded scenes.
        /// </summary>
        public UnloadOptions Options = new();

        /// <summary>
        /// </summary>
        public SceneUnloadData() { }

        /// <summary>
        /// </summary>
        /// <param name = "scene">Scene to unload.</param>
        public SceneUnloadData(Scene scene) : this(new Scene[] { scene }) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneName">Scene to unload by name.</param>
        public SceneUnloadData(string sceneName) : this(new string[] { sceneName }) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandle">Scene to unload by handle.</param>
        public SceneUnloadData(int sceneHandle) : this(new int[] { sceneHandle }) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneLookupData">Scene to unload by SceneLookupData.</param>
        public SceneUnloadData(SceneLookupData sceneLookupData)
        {
            SceneLookupDatas = new SceneLookupData[] { sceneLookupData };
        }

        /// <summary>
        /// </summary>
        /// <param name = "scenes">Scenes to unload.</param>
        public SceneUnloadData(List<Scene> scenes) : this(scenes.ToArray()) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneNames">Scenes to unload by names.</param>
        public SceneUnloadData(List<string> sceneNames) : this(sceneNames.ToArray()) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandles">Scenes to unload by handles.</param>
        public SceneUnloadData(List<int> sceneHandles) : this(sceneHandles.ToArray()) { }

        /// <summary>
        /// </summary>
        /// <param name = "scenes">Scenes to unload.</param>
        public SceneUnloadData(Scene[] scenes)
        {
            SceneLookupDatas = SceneLookupData.CreateData(scenes);
        }

        /// <summary>
        /// </summary>
        /// <param name = "sceneNames">Scenes to unload by names.</param>
        public SceneUnloadData(string[] sceneNames)
        {
            SceneLookupDatas = SceneLookupData.CreateData(sceneNames);
        }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandles">Scenes to unload by handles.</param>
        public SceneUnloadData(int[] sceneHandles)
        {
            SceneLookupDatas = SceneLookupData.CreateData(sceneHandles);
        }

        /// <summary>
        /// </summary>
        /// <param name = "sceneLookupDatas">Scenes to unload by SceneLookupDatas.</param>
        public SceneUnloadData(SceneLookupData[] sceneLookupDatas)
        {
            SceneLookupDatas = sceneLookupDatas;
        }

        /// <summary>
        /// Returns if any data is invalid, such as null entries.
        /// </summary>
        /// <returns></returns>
        internal bool DataInvalid()
        {
            // Null values.
            if (Params == null || SceneLookupDatas == null || Options == null)
                return true;
            // No lookups.
            if (SceneLookupDatas.Length == 0)
                return true;

            return false;
        }
    }
}