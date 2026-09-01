using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types
{
    /// <summary>
    /// Creates a singleton instance of a scriptable object.
    /// </summary>
    /// <typeparam name = "T"></typeparam>
    public abstract class SingletonScriptableObject<T> : ScriptableObject where T : ScriptableObject
    {
        private static T _instance = null;
        public static T Instance
        {
            get
            {
                if (_instance == null)
                {
                    T[] results = Resources.FindObjectsOfTypeAll<T>();
                    if (results.Length == 0)
                    {
                        Debug.LogError("SingletonScriptableObject: results length is 0 of " + typeof(T).ToString());
                        return null;
                    }
                    if (results.Length > 1)
                    {
                        Debug.LogError("SingletonScriptableObject: results length is greater than 1 of " + typeof(T).ToString());
                        return null;
                    }

                    _instance = results[0];
                    _instance.hideFlags = HideFlags.DontUnloadUnusedAsset;
                }

                return _instance;
            }
        }
    }
}