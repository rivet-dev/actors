#if UNITY_EDITOR
using FishNet.Managing;
using System.Collections.Generic;
using UnityEditor;
using UnityEditor.Build;
using UnityEngine;

namespace FishNet
{
    internal static class ScriptingDefines
    {
        [InitializeOnLoadMethod]
        public static void AddDefineSymbols()
        {
            // Get data about current target group
            bool standaloneAndServer = false;
            BuildTarget buildTarget = EditorUserBuildSettings.activeBuildTarget;
            BuildTargetGroup buildTargetGroup = BuildPipeline.GetBuildTargetGroup(buildTarget);
            if (buildTargetGroup == BuildTargetGroup.Standalone)
            {
                StandaloneBuildSubtarget standaloneSubTarget = EditorUserBuildSettings.standaloneBuildSubtarget;
                if (standaloneSubTarget == StandaloneBuildSubtarget.Server)
                    standaloneAndServer = true;
            }

            // Prepare named target, depending on above stuff
            NamedBuildTarget namedBuildTarget;
            if (standaloneAndServer)
                namedBuildTarget = NamedBuildTarget.Server;
            else
                namedBuildTarget = NamedBuildTarget.FromBuildTargetGroup(buildTargetGroup);

            string currentDefines = PlayerSettings.GetScriptingDefineSymbols(namedBuildTarget);
            /* Convert current defines into a hashset. This is so we can
             * determine if any of our defines were added. Only save playersettings
             * when a define is added. */
            HashSet<string> definesHs = new();
            string[] currentArr = currentDefines.Split(';');
            // Add current defines into hs.
            foreach (string item in currentArr)
                definesHs.Add(item);

            string proDefine = "FISHNET_PRO";
            string versionPrefix = "FISHNET_V";
            string[] currentVersionSplit = NetworkManager.FISHNET_VERSION.Split(".");
            string thisVersion = $"{versionPrefix}{currentVersionSplit[0]}";

            string[] fishNetDefines = new string[]
            {
                "FISHNET",
                thisVersion,
            };
            bool modified = false;
            // Now add FN defines.
            foreach (string item in fishNetDefines)
                modified |= definesHs.Add(item);

            // Remove old prediction defines.
            modified |= definesHs.Remove("PREDICTION_V2");
            /* Remove pro define if not on pro. This might look a little
             * funny because the code below varies depending on if pro or not. */
#pragma warning disable CS0162 // Unreachable code detected
                modified |= definesHs.Remove(proDefine);
#pragma warning restore CS0162 // Unreachable code detected

            List<string> definesToRemove = new();
            int versionPrefixLength = versionPrefix.Length;
            // Remove old versions.
            foreach (string item in definesHs)
            {
                // Do not remove this version.
                if (item == thisVersion)
                    continue;

                // If length is possible to be a version prefix and is so then remove it.
                if (item.Length >= versionPrefixLength && item.Substring(0, versionPrefixLength) == versionPrefix)
                    definesToRemove.Add(item);
            }

            modified |= definesToRemove.Count > 0;
            foreach (string item in definesToRemove)
            {
                definesHs.Remove(item);
                Debug.Log($"Removed unused Fish-Networking define {item}.");
            }

            if (modified)
            {
                Debug.Log("Added or removed Fish-Networking defines within player settings.");
                string changedDefines = string.Join(";", definesHs);
                PlayerSettings.SetScriptingDefineSymbols(namedBuildTarget, changedDefines);
            }
        }
    }
}
#endif