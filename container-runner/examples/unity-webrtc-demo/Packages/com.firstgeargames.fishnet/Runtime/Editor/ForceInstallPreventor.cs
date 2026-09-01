#if UNITY_EDITOR
using System;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace FishNet.Editing
{
    /* When you import playeveryware's EOS asset, it force installs NGO, which creates
     * a lot of issues for anyone not using NGO. This script will block the force installation. */
    public class ForceInstallPreventor : AssetPostprocessor
    {
        private static void OnPostprocessAllAssets(string[] importedAssets, string[] deletedAssets, string[] movedAssets, string[] movedFromAssetPaths)
        {
            // No need to continue if nothing was imported.
            if (importedAssets == null || importedAssets.Length == 0)
                return;

            EditorApplication.LockReloadAssemblies();

            foreach (string path in importedAssets)
                CheckTargetPath(path);

            /* We don't have a way to know if the user intentionally
             * had domain locked so we just have to unlock it and hope
             * we aren't messing up users settings.
             *
             * Worse case scenario this will only happen when the forceware
             * is removed.
             *
             * There is a 'didDomainReload' boolean override for this
             * method, but it does not seem to reflect the information
             * we need.
             * */
            EditorApplication.UnlockReloadAssemblies();
        }

        private void OnPreprocessAsset()
        {
            CheckTargetPath(assetImporter.assetPath);
        }

        private static void CheckTargetPath(string path)
        {
            if (!path.Contains("PackageInstallHelper_Netcode", StringComparison.CurrentCultureIgnoreCase))
                return;

            try
            {
                File.Delete(path);
                Debug.Log($"Fish-Networking prevented PlayEveryWare from forcefully installing Netcode for GameObjects.");
            }
            finally { }
        }
    }
}
#endif