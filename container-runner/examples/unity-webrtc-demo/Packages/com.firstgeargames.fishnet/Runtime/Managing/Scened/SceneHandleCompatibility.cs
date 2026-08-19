using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    internal static class SceneHandleCompatibility
    {
        public static int ToInt(Scene scene)
        {
#if UNITY_6000_5_OR_NEWER
            return unchecked((int)scene.handle.GetRawData());
#else
            return scene.handle;
#endif
        }

        public static bool HasHandle(Scene scene)
        {
            return ToInt(scene) != 0;
        }

        public static bool Equals(Scene scene, int handle)
        {
#if UNITY_6000_5_OR_NEWER
            return scene.handle == SceneHandle.FromRawData(unchecked((ulong)handle));
#else
            return scene.handle == handle;
#endif
        }

        public static bool Equals(Scene a, Scene b)
        {
#if UNITY_6000_5_OR_NEWER
            return a.handle == b.handle;
#else
            return a.handle == b.handle;
#endif
        }
    }
}
