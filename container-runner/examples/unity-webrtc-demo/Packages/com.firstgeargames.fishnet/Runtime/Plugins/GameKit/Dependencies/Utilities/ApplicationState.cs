using UnityEngine;
#if UNITY_EDITOR
using UnityEditor;
#endif

namespace GameKit.Dependencies.Utilities
{
#if UNITY_EDITOR
    [InitializeOnLoad]
#endif
    public static class ApplicationState
    {
#if !UNITY_EDITOR
        /// <summary>
        /// True if application is quitting.
        /// </summary>
        private static bool _isQuitting;
#endif
        static ApplicationState()
        {
#if !UNITY_EDITOR
            _isQuitting = false;
#endif
            Application.quitting -= Application_quitting;
            Application.quitting += Application_quitting;
        }

        private static void Application_quitting()
        {
#if !UNITY_EDITOR
            _isQuitting = true;
#endif
        }

        /// <summary>
        /// Returns if the application is quitting for editor or builds.
        /// </summary>
        /// <returns></returns>
        public static bool IsQuitting()
        {
#if UNITY_EDITOR
            if ((!EditorApplication.isPlayingOrWillChangePlaymode && EditorApplication.isPlaying) || !EditorApplication.isPlaying)
                return true;
            else
                return false;
#else
            return _isQuitting;
#endif
        }

        /// <summary>
        /// Returns if the application is playing for editor or builds.
        /// </summary>
        /// <returns></returns>
        public static bool IsPlaying()
        {
#if UNITY_EDITOR
            return EditorApplication.isPlaying;
#else
            return Application.isPlaying;
#endif
        }

        /// <summary>
        /// Quits the application for editor or builds.
        /// </summary>
        public static void Quit()
        {
#if UNITY_EDITOR
            EditorApplication.isPlaying = false;
#else
            Application.Quit();
#endif
        }
    }
}