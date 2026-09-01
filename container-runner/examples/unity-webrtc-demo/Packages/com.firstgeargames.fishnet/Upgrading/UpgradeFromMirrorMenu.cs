#if UNITY_EDITOR
using FishNet.Documenting;
using System.Collections.Generic;
using UnityEditor;
using UnityEngine;

namespace FishNet.Upgrading.Mirror.Editing
{
    /* IMPORTANT IMPORTANT IMPORTANT IMPORTANT
     * If you receive errors about missing Mirror components,
     * such as NetworkIdentity, then remove MIRROR and any other
     * MIRROR defines.
     * Project Settings -> Player -> Other -> Scripting Define Symbols.
     *
     * If you are also using my assets add FGG_ASSETS to the defines, and
     * then remove it after running this script. */
    [APIExclude]
    public class UpgradeFromMirrorMenu : MonoBehaviour
    {
        /// <summary>
        /// Replaces all components.
        /// </summary>
        [MenuItem("Tools/Fish-Networking/Utility/Upgrading/From Mirror/Replace Components", false, 1)]
        private static void ReplaceComponents()
        {
#if MIRROR
            MirrorUpgrade result = GameObject.FindObjectOfType<MirrorUpgrade>();
            if (result != null)
            {
                Debug.LogError("MirrorUpgrade already exist in the scene. This suggests an operation is currently running.");
                return;
            }

            GameObject iteratorGo = new GameObject();
            iteratorGo.AddComponent<MirrorUpgrade>();
#else
            Debug.LogError("Mirror must be imported to perform this function.");
#endif
        }

        [MenuItem("Tools/Fish-Networking/Utility/Upgrading/From Mirror/Remove Defines", false, 2)]
        private static void RemoveDefines()
        {
            string currentDefines = PlayerSettings.GetScriptingDefineSymbolsForGroup(EditorUserBuildSettings.selectedBuildTargetGroup);
            /* Convert current defines into a hashset. This is so we can
             * determine if any of our defines were added. Only save playersettings
             * when a define is added. */
            HashSet<string> definesHs = new();
            string[] currentArr = currentDefines.Split(';');

            bool removed = false;
            // Add any define which doesn't contain MIRROR.
            foreach (string item in currentArr)
            {
                string itemLower = item.ToLower();
                if (itemLower != "mirror" && !itemLower.StartsWith("mirror_"))
                    definesHs.Add(item);
                else
                    removed = true;
            }

            if (removed)
            {
                Debug.Log("Removed Mirror defines to player settings.");
                string changedDefines = string.Join(";", definesHs);
                PlayerSettings.SetScriptingDefineSymbolsForGroup(EditorUserBuildSettings.selectedBuildTargetGroup, changedDefines);
            }
        }
    }
}
#endif