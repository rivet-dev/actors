using UnityEngine;

namespace FishNet.Configuring.EditorCloning
{
    public static class CloneChecker
    {
        /// <summary>
        /// Returns true if this editor is a multiplayer clone.
        /// </summary>
        /// <returns></returns>
        public static bool IsMultiplayerClone(out EditorCloneType editorCloneType)
        {
            if (IsUnityMultiplayerModeClone())
            {
                editorCloneType = EditorCloneType.UnityMultiplayer;
                return true;
            }

            if (IsParrelSyncClone())
            {
                editorCloneType = EditorCloneType.ParrelSync;
                return true;
            }

            editorCloneType = EditorCloneType.None;

            return false;
        }

        /// <summary>
        /// Returns true if ParrelSync clone with file modification enabled, or if not a clone.
        /// </summary>
        /// <returns></returns>
        public static bool CanGenerateFiles()
        {
            //Not a clone.
            if (!IsMultiplayerClone(out EditorCloneType cloneType))
                return true;

            //A clone, but not parrelsync.
            if (cloneType != EditorCloneType.ParrelSync)
                return false;

            return CanParrelSyncSetData();
        }

        /// <summary>
        /// Uses preprocessors to determine if ParrelSync and can set data.
        /// </summary>
        /// <returns></returns>
        private static bool CanParrelSyncSetData()
        {
            #if PARRELSYNC && UNITY_EDITOR

            bool areSetsBlocked = ParrelSync.Preferences.AssetModPref.Value;

            return !areSetsBlocked;
            
            #else
            
            return false;

            #endif
        }

        /// <summary>
        /// Returns true if is a ParrelSync clone.
        /// </summary>
        public static bool IsParrelSyncClone()
        {
            #if PARRELSYNC && UNITY_EDITOR

            return ParrelSync.ClonesManager.IsClone();

            #else
            
            return false;
            
            #endif
        }

        /// <summary>
        /// Returns true if a Unity MultiplayerMode clone.
        /// </summary>
        /// <returns></returns>
        public static bool IsUnityMultiplayerModeClone()
        {
            return Application.dataPath.ToLower().Contains("library/vp/");
        }
    }
}