using FishNet.Object;
using FishNet.Serializing.Helping;
using System.Collections.Generic;
using System.IO;
using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Data about which scenes to load.
    /// </summary>
    public class SceneLoadData
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
        /// NetworkObjects to move to the new scenes. Objects will be moved to the first scene.
        /// </summary>
        public NetworkObject[] MovedNetworkObjects = new NetworkObject[0];
        /// <summary>
        /// How to replace current scenes with new ones. When replacing scenes the first scene loaded will be set as the active scene, and the rest additive.
        /// </summary>
        public ReplaceOption ReplaceScenes = ReplaceOption.None;
        /// <summary>
        /// Parameters which may be set and will be included in load callbacks.
        /// </summary>
        public LoadParams Params = new();
        /// <summary>
        /// Additional options to use for loaded scenes.
        /// </summary>
        public LoadOptions Options = new();
        public SceneLoadData() { }

        /// <summary>
        /// </summary>
        /// <param name = "scene">Scene to load.</param>
        public SceneLoadData(Scene scene) : this(new Scene[] { scene }, null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneName">Scene to load by name.</param>
        public SceneLoadData(string sceneName) : this(new string[] { sceneName }, null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandle">Scene to load by handle.</param>
        public SceneLoadData(int sceneHandle) : this(new int[] { sceneHandle }, null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandle">Scene to load by handle.</param>
        /// <param name = "sceneName">Scene to load by name.</param>
        public SceneLoadData(int sceneHandle, string sceneName) : this(new SceneLookupData(sceneHandle, sceneName)) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneLookupData">Scene to load by SceneLookupData.</param>
        public SceneLoadData(SceneLookupData sceneLookupData) : this(new SceneLookupData[] { sceneLookupData }) { }

        /// <summary>
        /// </summary>
        /// <param name = "scenes">Scenes to load.</param>
        public SceneLoadData(List<Scene> scenes) : this(scenes.ToArray(), null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneNames">Scenes to load by name.</param>
        public SceneLoadData(List<string> sceneNames) : this(sceneNames.ToArray(), null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandles">Scenes to load by handle.</param>
        public SceneLoadData(List<int> sceneHandles) : this(sceneHandles.ToArray(), null) { }

        /// <summary>
        /// </summary>
        /// <param name = "scenes">Scenes to load.</param>
        public SceneLoadData(Scene[] scenes) : this(scenes, null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneNames">Scenes to load by name.</param>
        public SceneLoadData(string[] sceneNames) : this(sceneNames, null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandles">Scenes to load by handle.</param>
        public SceneLoadData(int[] sceneHandles) : this(sceneHandles, null) { }

        /// <summary>
        /// </summary>
        /// <param name = "sceneLookupDatas">Scenes to load by SceneLookupDatas.</param>
        public SceneLoadData(SceneLookupData[] sceneLookupDatas) : this(sceneLookupDatas, null) { }

        /// <summary>
        /// </summary>
        /// <param name = "scene">Scene to load.</param>
        /// <param name = "movedNetworkObjects">NetworkObjects to move to the first specified scene.</param>
        public SceneLoadData(Scene scene, NetworkObject[] movedNetworkObjects)
        {
            SceneLookupData data = SceneLookupData.CreateData(scene);
            Construct(new SceneLookupData[] { data }, movedNetworkObjects);
        }

        /// <summary>
        /// </summary>
        /// <param name = "scenes">Scenes to load.</param>
        /// <param name = "movedNetworkObjects">NetworkObjects to move to the first specified scene.</param>
        public SceneLoadData(Scene[] scenes, NetworkObject[] movedNetworkObjects)
        {
            SceneLookupData[] datas = SceneLookupData.CreateData(scenes);
            Construct(datas, movedNetworkObjects);
        }

        /// <summary>
        /// </summary>
        /// <param name = "sceneNames">Scenes to load by Name.</param>
        /// <param name = "movedNetworkObjects">NetworkObjects to move to the first specified scene.</param>
        public SceneLoadData(string[] sceneNames, NetworkObject[] movedNetworkObjects)
        {
            SceneLookupData[] datas = SceneLookupData.CreateData(sceneNames);
            Construct(datas, movedNetworkObjects);
        }

        /// <summary>
        /// </summary>
        /// <param name = "sceneHandles">Scenes to load by handle.</param>
        /// <param name = "movedNetworkObjects">NetworkObjects to move to the first specified scene.</param>
        public SceneLoadData(int[] sceneHandles, NetworkObject[] movedNetworkObjects)
        {
            SceneLookupData[] datas = SceneLookupData.CreateData(sceneHandles);
            Construct(datas, movedNetworkObjects);
        }

        /// <summary>
        /// </summary>
        /// <param name = "sceneLookupDatas">Scenes to load by SceneLookupDatas.</param>
        /// <param name = "movedNetworkObjects">NetworkObjects to move to the first specified scene.</param>
        public SceneLoadData(SceneLookupData[] sceneLookupDatas, NetworkObject[] movedNetworkObjects)
        {
            sceneLookupDatas = SceneLookupData.ValidateData(sceneLookupDatas);
            Construct(sceneLookupDatas, movedNetworkObjects);
        }

        /// <summary>
        /// Called at the end of every constructor.
        /// </summary>
        private void Construct(SceneLookupData[] datas, NetworkObject[] movedNetworkObjects)
        {
            SceneLookupDatas = datas;
            if (movedNetworkObjects == null)
                movedNetworkObjects = new NetworkObject[0];
            MovedNetworkObjects = movedNetworkObjects;
        }

        /// <summary>
        /// Gets the first Scene in SceneLookupDatas.
        /// </summary>
        /// <returns></returns>
        public Scene GetFirstLookupScene()
        {
            foreach (SceneLookupData sld in SceneLookupDatas)
            {
                Scene result = sld.GetScene(out _, false);
                if (!string.IsNullOrEmpty(result.name))
                    return result;
            }

            return default;
        }

        /// <summary>
        /// Returns if any data is invalid, such as null entries.
        /// </summary>
        /// <returns></returns>
        internal bool DataInvalid()
        {
            // Null values.
            if (Params == null || MovedNetworkObjects == null || SceneLookupDatas == null || Options == null)
                return true;
            // No lookups.
            if (SceneLookupDatas.Length == 0)
                return true;

            return false;
        }
    }
}