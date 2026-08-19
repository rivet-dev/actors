using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    public struct UnloadedScene
    {
        public readonly string Name;
        public readonly int Handle;

        public UnloadedScene(Scene s)
        {
            Name = s.name;
            Handle = SceneHandleCompatibility.ToInt(s);
        }

        public UnloadedScene(string name, int handle)
        {
            Name = name;
            Handle = handle;
        }

        /// <summary>
        /// Returns a scene based on handle.
        /// Result may not be valid as some Unity versions discard of the scene information after unloading.
        /// </summary>
        /// <returns></returns>
        public Scene GetScene()
        {
            int loadedScenes = UnityEngine.SceneManagement.SceneManager.sceneCount;
            for (int i = 0; i < loadedScenes; i++)
            {
                Scene s = UnityEngine.SceneManagement.SceneManager.GetSceneAt(i);
                if (s.IsValid() && SceneHandleCompatibility.Equals(s, Handle))
                    return s;
            }

            return default;
        }
    }
}
