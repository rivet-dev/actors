#if UNITY_EDITOR
using System;
using UnityEditor;
using UnityEngine;

namespace FishNet.Editing
{
    [InitializeOnLoad]
    public class PlayModeTracker
    {
        static PlayModeTracker()
        {
            EditorApplication.playModeStateChanged += OnPlayModeStateChanged;
        }

        ~PlayModeTracker()
        {
            EditorApplication.playModeStateChanged -= OnPlayModeStateChanged;
        }

        /// <summary>
        /// DateTime when the editor last exited playmode.
        /// </summary>
        private static DateTime _quitTime = DateTime.MaxValue;

        /// <summary>
        /// True if the editor has exited playmode within past.
        /// </summary>
        /// <param name = "past"></param>
        /// <returns></returns>
        internal static bool QuitRecently(float past)
        {
            past *= 1000;
            return (DateTime.Now - _quitTime).TotalMilliseconds < past;
        }

        private static void OnPlayModeStateChanged(PlayModeStateChange stateChange)
        {
            switch (stateChange)
            {
                case PlayModeStateChange.ExitingPlayMode:
                    _quitTime = DateTime.Now;
                    break;
            }
        }
    }
}

#endif