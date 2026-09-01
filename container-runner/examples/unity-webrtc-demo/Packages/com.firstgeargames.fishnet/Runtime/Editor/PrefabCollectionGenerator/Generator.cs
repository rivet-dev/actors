#if UNITY_EDITOR
using System;
using FishNet.Configuring;
using FishNet.Managing;
using FishNet.Managing.Object;
using FishNet.Object;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using FishNet.Configuring.EditorCloning;
using UnityEditor;
using UnityEngine;
using UnityDebug = UnityEngine.Debug;

namespace FishNet.Editing.PrefabCollectionGenerator
{
    internal sealed class Generator : AssetPostprocessor
    {
        public Generator()
        {
            if (!_subscribed)
            {
                _subscribed = true;
                EditorApplication.update += OnEditorUpdate;
            }
        }

        ~Generator()
        {
            if (_subscribed)
            {
                _subscribed = false;
                EditorApplication.update -= OnEditorUpdate;
            }
        }

        #region Types.
        internal readonly struct SpecifiedFolder
        {
            public readonly string Path;
            public readonly bool Recursive;

            public SpecifiedFolder(string path, bool recursive)
            {
                Path = path;
                Recursive = recursive;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// True to ignore post process changes.
        /// </summary>
        public static bool IgnorePostProcess = false;
        #endregion

        #region Private.
        /// <summary>
        /// Last asset to import when there was only one imported asset and no other changes.
        /// </summary>
        private static string _lastSingleImportedAsset = string.Empty;
        /// <summary>
        /// Cached DefaultPrefabObjects reference.
        /// </summary>
        private static DefaultPrefabObjects _cachedDefaultPrefabs;
        /// <summary>
        /// True to refresh prefabs next update.
        /// </summary>
        private static bool _retryRefreshDefaultPrefabs;
        /// <summary>
        /// True if already subscribed to EditorApplication.Update.
        /// </summary>
        private static bool _subscribed;
        /// <summary>
        /// True if ran once since editor started.
        /// </summary>
        [System.NonSerialized]
        private static bool _ranOnce;
        /// <summary>
        /// Last paths of updated nobs during a changed update.
        /// </summary>
        [System.NonSerialized]
        private static List<string> _lastUpdatedNamePaths = new();
        /// <summary>
        /// Last frame changed was updated.
        /// </summary>
        [System.NonSerialized]
        private static int _lastUpdatedFrame = -1;
        /// <summary>
        /// Length of assets strings during the last update.
        /// </summary>
        [System.NonSerialized]
        private static int _lastUpdatedLengths = -1;
        #endregion

        internal static string[] GetProjectFiles(string startingPath, string fileExtension, List<string> excludedPaths, bool recursive)
        {
            // starting path is excluded.
            if (excludedPaths.Contains(startingPath))
                return new string[0];

            // Folders remaining to be iterated.
            List<string> enumeratedCollection = new() { startingPath };
            // Only check other directories if recursive.
            if (recursive)
            {
                // Find all folders which aren't excluded.
                for (int i = 0; i < enumeratedCollection.Count; i++)
                {
                    string[] allFolders = Directory.GetDirectories(enumeratedCollection[i], "*", SearchOption.TopDirectoryOnly);
                    for (int z = 0; z < allFolders.Length; z++)
                    {
                        string current = allFolders[z];
                        // Not excluded.
                        if (!excludedPaths.Contains(current))
                            enumeratedCollection.Add(current);
                    }
                }
            }

            // Valid prefab files.
            List<string> results = new();
            // Build files from folders.
            int count = enumeratedCollection.Count;
            for (int i = 0; i < count; i++)
            {
                string[] r = Directory.GetFiles(enumeratedCollection[i], "*.prefab", SearchOption.TopDirectoryOnly);
                results.AddRange(r);
            }

            return results.ToArray();
        }

        /// <summary>
        /// Returns a message to attach to logs if objects were dirtied.
        /// </summary>
        private static string GetDirtiedMessage(PrefabGeneratorConfigurations settings, bool dirtied)
        {
            if (!settings.SaveChanges && dirtied)
                return " One or more NetworkObjects were dirtied. Please save your project.";
            else
                return string.Empty;
        }

        /// <summary>
        /// Updates prefabs by using only changed information.
        /// </summary>
        public static void GenerateChanged(string[] importedAssets, string[] deletedAssets, string[] movedAssets, string[] movedFromAssetPaths, PrefabGeneratorConfigurations settings = null)
        {
            if (!CloneChecker.CanGenerateFiles())
            {
                UnityDebug.Log("Skipping prefab generation as clone settings does not allow it.");
                return;
            }

            // Do not run if the reserializer is currently running.
            if (ReserializeNetworkObjectsEditor.IsRunning)
                return;

            if (settings == null)
                settings = Configuration.Configurations.PrefabGenerator;
            if (!settings.Enabled)
                return;

            bool log = settings.LogToConsole;
            Stopwatch sw = log ? Stopwatch.StartNew() : null;

            DefaultPrefabObjects prefabCollection = GetDefaultPrefabObjects(settings);
            // No need to error if nto found, GetDefaultPrefabObjects will.
            if (prefabCollection == null)
                return;

            int assetsLength = importedAssets.Length + deletedAssets.Length + movedAssets.Length + movedFromAssetPaths.Length;
            List<string> changedNobPaths = new();

            System.Type goType = typeof(GameObject);
            IterateAssetCollection(importedAssets);
            IterateAssetCollection(movedAssets);

            // True if dirtied by changes.
            bool dirtied;
            // First remove null entries.
            int startCount = prefabCollection.GetObjectCount();
            prefabCollection.RemoveNull();
            dirtied = prefabCollection.GetObjectCount() != startCount;
            // First index which new objects will be added to.
            int firstAddIndex = prefabCollection.GetObjectCount() - 1;

            // Iterates strings adding prefabs to collection.
            void IterateAssetCollection(string[] c)
            {
                foreach (string item in c)
                {
                    System.Type assetType = AssetDatabase.GetMainAssetTypeAtPath(item);
                    if (assetType != goType)
                        continue;

                    NetworkObject nob = AssetDatabase.LoadAssetAtPath<NetworkObject>(item);
                    if (CanAddNetworkObject(nob, settings))
                    {
                        changedNobPaths.Add(item);
                        prefabCollection.AddObject(nob, true);
                        dirtied = true;
                    }
                }
            }

            // To prevent out of range.
            if (firstAddIndex < 0 || firstAddIndex >= prefabCollection.GetObjectCount())
                firstAddIndex = 0;
            dirtied |= prefabCollection.SetAssetPathHashes(firstAddIndex);

            if (log && dirtied)
                UnityDebug.Log($"Default prefab generator updated prefabs in {sw.ElapsedMilliseconds}ms.{GetDirtiedMessage(settings, dirtied)}");

            // Check for redundancy.
            int frameCount = Time.frameCount;
            int changedCount = changedNobPaths.Count;
            if (frameCount == _lastUpdatedFrame && assetsLength == _lastUpdatedLengths && changedCount == _lastUpdatedNamePaths.Count && changedCount > 0)
            {
                bool allMatch = true;
                for (int i = 0; i < changedCount; i++)
                {
                    if (changedNobPaths[i] != _lastUpdatedNamePaths[i])
                    {
                        allMatch = false;
                        break;
                    }
                }

                /* If the import results are the same as the last attempt, on the same frame
                 * then there is likely an issue saving the assets. */
                if (allMatch)
                {
                    // Unset dirtied to prevent a save.
                    dirtied = false;
                    // Log this no matter what, it's critical.
                    UnityDebug.LogError($"Default prefab generator had a problem saving one or more assets. " + $"This usually occurs when the assets cannot be saved due to missing scripts or serialization errors. " + $"Please see above any prefabs which could not save any make corrections.");
                }
            }
            // Set last values.
            _lastUpdatedFrame = Time.frameCount;
            _lastUpdatedNamePaths = changedNobPaths;
            _lastUpdatedLengths = assetsLength;

            EditorUtility.SetDirty(prefabCollection);
            if (dirtied && settings.SaveChanges)
                AssetDatabase.SaveAssets();
        }

        /// <summary>
        /// Gets NetworkObjects from folders using settings.
        /// </summary>
        internal static List<NetworkObject> GetNetworkObjects(PrefabGeneratorConfigurations settings = null)
        {
            List<SpecifiedFolder> folders = GetSpecifiedFolders(settings);
            if (folders == null)
                return new();

            return GetNetworkObjects(folders, settings);
        }

        /// <summary>
        /// Gets specified folders to check for prefab generation. This may include excluded paths. Null is returned on error.
        /// </summary>
        internal static List<SpecifiedFolder> GetSpecifiedFolders(PrefabGeneratorConfigurations settings = null)
        {
            settings = GetSettingsIfNull(settings);

            List<string> folderStrs;

            if (settings.SearchScope == (int)SearchScopeType.EntireProject)
            {
                folderStrs = new();
                folderStrs.Add("Assets*");
            }
            else if (settings.SearchScope == (int)SearchScopeType.SpecificFolders)
            {
                folderStrs = settings.IncludedFolders.ToList();
            }
            else
            {
                UnityDebug.LogError($"{settings.SearchScope} is not handled; folder paths cannot be found.");
                return null;
            }

            return GetSpecifiedFolders(folderStrs);
        }

        /// <summary>
        /// Gets all NetworkObjects in specified folders while ignoring any excluded paths.
        /// </summary>
        internal static List<NetworkObject> GetNetworkObjects(SpecifiedFolder specifiedFolder, PrefabGeneratorConfigurations settings = null)
        {
            List<string> excludedPaths = GetSettingsIfNull(settings).ExcludedFolders;

            List<NetworkObject> foundNobs = new();

            foreach (string path in GetProjectFiles(specifiedFolder.Path, "prefab", excludedPaths, specifiedFolder.Recursive))
            {
                NetworkObject nob = AssetDatabase.LoadAssetAtPath<NetworkObject>(path);
                if (CanAddNetworkObject(nob, settings))
                    foundNobs.Add(nob);
            }

            return foundNobs;
        }

        /// <summary>
        /// Gets all NetworkObjects in specified folders while ignoring any excluded paths.
        /// </summary>
        internal static List<NetworkObject> GetNetworkObjects(List<SpecifiedFolder> specifiedFolders, PrefabGeneratorConfigurations settings = null)
        {
            List<NetworkObject> foundNobs = new();

            foreach (SpecifiedFolder sf in specifiedFolders)
                foundNobs.AddRange(GetNetworkObjects(sf, settings));

            foundNobs = foundNobs.OrderBy(nob => nob.AssetPathHash).ToList();

            return foundNobs;
        }

        /// <summary>
        /// Generates prefabs by iterating all files within settings parameters.
        /// </summary>
        public static void GenerateFull(PrefabGeneratorConfigurations settings = null, bool forced = false, bool initializeAdded = true)
        {
            if (!CloneChecker.CanGenerateFiles())
            {
                UnityDebug.Log("Skipping prefab generation as clone settings does not allow it.");
                return;
            }

            // Do not run if the re-serializer is currently running.
            if (ReserializeNetworkObjectsEditor.IsRunning)
            {
                UnityDebug.LogError($"Cannot generate default prefabs when ReserializeNetworkObjectsEditor is running");
                return;
            }

            settings = GetSettingsIfNull(settings);

            if (!forced && !settings.Enabled)
                return;

            bool log = settings.LogToConsole;

            Stopwatch sw = log ? Stopwatch.StartNew() : null;

            DefaultPrefabObjects prefabCollection = GetDefaultPrefabObjects(settings);
            // No need to error if not found, GetDefaultPrefabObjects will throw.
            if (prefabCollection == null)
                return;

            List<NetworkObject> foundNobs = GetNetworkObjects(settings);

            // Clear and add built list.
            prefabCollection.Clear();
            prefabCollection.AddObjects(foundNobs, checkForDuplicates: false, initializeAdded);
            bool dirtied = prefabCollection.SetAssetPathHashes(0);

            int newCount = prefabCollection.GetObjectCount();
            if (log)
            {
                string dirtiedMessage = newCount > 0 ? GetDirtiedMessage(settings, dirtied) : string.Empty;
                UnityDebug.Log($"Default prefab generator found {newCount} prefabs in {sw.ElapsedMilliseconds}ms.{dirtiedMessage}");
            }
            // Only set dirty if and save if prefabs were found.
            if (newCount > 0)
            {
                EditorUtility.SetDirty(prefabCollection);
                if (settings.SaveChanges)
                    AssetDatabase.SaveAssets();
            }
        }

        /// <summary>
        /// Returns settings parameter if not null, otherwise returns settings from configuration.
        /// </summary>
        internal static PrefabGeneratorConfigurations GetSettingsIfNull(PrefabGeneratorConfigurations settings) => settings == null ? Configuration.Configurations.PrefabGenerator : settings;

        /// <summary>
        /// Iterates folders building them into SpecifiedFolders.
        /// </summary>
        internal static List<SpecifiedFolder> GetSpecifiedFolders(List<string> strFolders)
        {
            List<SpecifiedFolder> results = new();
            // Remove astericks.
            foreach (string path in strFolders)
            {
                int pLength = path.Length;
                if (pLength == 0)
                    continue;

                bool recursive;
                string p;
                // If the last character indicates resursive.
                if (path.Substring(pLength - 1, 1) == "*")
                {
                    p = path.Substring(0, pLength - 1);
                    recursive = true;
                }
                else
                {
                    p = path;
                    recursive = false;
                }

                p = GetPlatformPath(p);

                // Path does not exist.
                if (!Directory.Exists(p))
                    continue;

                results.Add(new(p, recursive));
            }


            RemoveOverlappingFolders(results);

            return results;

            // Removes paths which may overlap each other, such as sub directories.
            static void RemoveOverlappingFolders(List<SpecifiedFolder> specifiedFolders)
            {
                for (int z = 0; z < specifiedFolders.Count; z++)
                {
                    for (int i = 0; i < specifiedFolders.Count; i++)
                    {
                        // Do not check against self.
                        if (i == z)
                            continue;

                        string zPath = GetPathWithSeparator(specifiedFolders[z].Path);
                        string iPath = GetPathWithSeparator(specifiedFolders[i].Path);

                        // Duplicate.
                        if (zPath.Equals(iPath, System.StringComparison.OrdinalIgnoreCase))
                        {
                            UnityDebug.LogError($"The same path is specified multiple times in the DefaultPrefabGenerator settings. Remove the duplicate to clear this error.");
                            specifiedFolders.RemoveAt(i);
                            break;
                        }

                        /* We are checking if i can be within
                         * z. This is only possible if i is longer
                         * than z. */
                        if (iPath.Length < zPath.Length)
                            continue;
                        /* Do not need to check if not recursive.
                         * Only recursive needs to be checked because
                         * a shorter recursive path could contain
                         * a longer path. */
                        if (!specifiedFolders[z].Recursive)
                            continue;

                        // // Compare paths.
                        // string zPath = GetPathWithSeparator(specifiedFolders[z].Path);
                        // string iPath = specifiedFolders[i].Path.Substring(0, zPath.Length);
                        string iPathRecursiveCheck = iPath.Substring(0, zPath.Length);
                        
                        // If paths match.
                        if (iPathRecursiveCheck.Equals(zPath, System.StringComparison.OrdinalIgnoreCase))
                        {
                            UnityDebug.LogError($"Path {specifiedFolders[i].Path} is included within recursive path {specifiedFolders[z].Path}. Remove path {specifiedFolders[i].Path} to clear this error.");
                            specifiedFolders.RemoveAt(i);
                            break;
                        }
                    }
                }

                string GetPathWithSeparator(string txt)
                {
                    return txt.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
                }
            }
        }

        internal static string GetPlatformPath(string path)
        {
            if (string.IsNullOrEmpty(path))
                return path;

            path = path.Replace(@"\"[0], Path.DirectorySeparatorChar);
            path = path.Replace(@"/"[0], Path.DirectorySeparatorChar);
            return path;
        }

        /// <summary>
        /// Returns the DefaultPrefabObjects file.
        /// </summary>
        internal static DefaultPrefabObjects GetDefaultPrefabObjects(PrefabGeneratorConfigurations settings = null)
        {
            if (settings == null)
                settings = Configuration.Configurations.PrefabGenerator;

            // If not using default prefabs then exit early.
            if (!settings.Enabled)
                return null;

            // Load the prefab collection 
            string defaultPrefabsPath = settings.DefaultPrefabObjectsPath_Platform;
            string fullDefaultPrefabsPath = defaultPrefabsPath.Length > 0 ? Path.GetFullPath(defaultPrefabsPath) : string.Empty;

            // If cached prefabs is not the same path as assetPath.
            if (_cachedDefaultPrefabs != null)
            {
                string unityAssetPath = AssetDatabase.GetAssetPath(_cachedDefaultPrefabs);
                string fullCachedPath = unityAssetPath.Length > 0 ? Path.GetFullPath(unityAssetPath) : string.Empty;
                if (fullCachedPath != fullDefaultPrefabsPath)
                    _cachedDefaultPrefabs = null;
            }

            // If cached is null try to get it.
            if (_cachedDefaultPrefabs == null)
            {
                // Only try to load it if file exist.
                if (File.Exists(fullDefaultPrefabsPath))
                {
                    _cachedDefaultPrefabs = AssetDatabase.LoadAssetAtPath<DefaultPrefabObjects>(defaultPrefabsPath);
                    if (_cachedDefaultPrefabs == null)
                    {
                        // If already retried then throw an error.
                        if (_retryRefreshDefaultPrefabs)
                        {
                            UnityDebug.LogError("DefaultPrefabObjects file exists but it could not be loaded by Unity. Use the Fish-Networking menu -> Utility -> Refresh Default Prefabs to refresh prefabs.");
                        }
                        else
                        {
                            UnityDebug.Log("DefaultPrefabObjects file exists but it could not be loaded by Unity. Trying to reload the file next frame.");
                            _retryRefreshDefaultPrefabs = true;
                        }
                        return null;
                    }
                }
            }

            // Will be true also if not a clone.
            if (CloneChecker.CanGenerateFiles())
                CreateDefaultPrefabsAssetIfNeeded();

            // Creates DefaultPrefabs asset if missing or not set.
            void CreateDefaultPrefabsAssetIfNeeded()
            {
                if (_cachedDefaultPrefabs == null)
                {
                    string fullPath = Path.GetFullPath(defaultPrefabsPath);
                    UnityDebug.Log($"Creating a new DefaultPrefabsObject at {fullPath}.");
                    string directory = Path.GetDirectoryName(fullPath);

                    if (!Directory.Exists(directory))
                    {
                        Directory.CreateDirectory(directory);
                        AssetDatabase.Refresh();
                    }

                    _cachedDefaultPrefabs = ScriptableObject.CreateInstance<DefaultPrefabObjects>();
                    AssetDatabase.CreateAsset(_cachedDefaultPrefabs, defaultPrefabsPath);
                    AssetDatabase.SaveAssets();
                }
            }

            if (_cachedDefaultPrefabs != null && _retryRefreshDefaultPrefabs)
                UnityDebug.Log("DefaultPrefabObjects found on the second iteration.");

            return _cachedDefaultPrefabs;
        }

        /// <summary>
        /// Called every frame the editor updates.
        /// </summary>
        private static void OnEditorUpdate()
        {
            if (!_retryRefreshDefaultPrefabs)
                return;

            GenerateFull();
            _retryRefreshDefaultPrefabs = false;
        }

        /// <summary>
        /// Called by Unity when assets are modified.
        /// </summary>
        private static void OnPostprocessAllAssets(string[] importedAssets, string[] deletedAssets, string[] movedAssets, string[] movedFromAssetPaths)
        {
            if (Application.isPlaying)
                return;
            // If retrying next frame don't bother updating, next frame will do a full refresh.
            if (_retryRefreshDefaultPrefabs)
                return;
            // Post process is being ignored. Could be temporary or user has disabled this feature.
            if (IgnorePostProcess)
                return;
            /* Don't iterate if updating or compiling as that could cause an infinite loop
             * due to the prefabs being generated during an update, which causes the update
             * to start over, which causes the generator to run again, which... you get the idea. */
            if (EditorApplication.isCompiling)
                return;

            DefaultPrefabObjects prefabCollection = GetDefaultPrefabObjects();
            if (prefabCollection == null)
                return;
            PrefabGeneratorConfigurations settings = Configuration.Configurations.PrefabGenerator;

            if (prefabCollection.GetObjectCount() == 0)
            {
                // If there are no prefabs then do a full rebuild. Odds of there being none are pretty much nill.
                GenerateFull(settings);
            }
            else
            {
                int totalChanges = importedAssets.Length + deletedAssets.Length + movedAssets.Length + movedFromAssetPaths.Length;
                // Nothing has changed. This shouldn't occur but unity is funny so we're going to check anyway.
                if (totalChanges == 0)
                    return;

                // Normalizes path.
                string dpoPath = Path.GetFullPath(settings.DefaultPrefabObjectsPath_Platform);
                // If total changes is 1 and the only changed file is the default prefab collection then do nothing.
                if (totalChanges == 1)
                {
                    // Do not need to check movedFromAssetPaths because that's not possible for this check.
                    if ((importedAssets.Length == 1 && Path.GetFullPath(importedAssets[0]) == dpoPath) || (deletedAssets.Length == 1 && Path.GetFullPath(deletedAssets[0]) == dpoPath) || (movedAssets.Length == 1 && Path.GetFullPath(movedAssets[0]) == dpoPath))
                        return;

                    /* If the only change is an import then check if the imported file
                     * is the same as the last, and if so check into returning early.
                     * For some reason occasionally when files are saved unity runs postprocess
                     * multiple times on the same file. */
                    string imported = importedAssets.Length == 1 ? importedAssets[0] : null;
                    if (imported != null && imported == _lastSingleImportedAsset)
                    {
                        // If here then the file is the same. Make sure it's already in the collection before returning.
                        System.Type assetType = AssetDatabase.GetMainAssetTypeAtPath(imported);
                        // Not a gameObject, no reason to continue.
                        if (assetType != typeof(GameObject))
                            return;

                        NetworkObject nob = AssetDatabase.LoadAssetAtPath<NetworkObject>(imported);
                        // If is a networked object.
                        if (CanAddNetworkObject(nob, settings))
                        {
                            // Already added!
                            if (prefabCollection.Prefabs.Contains(nob))
                                return;
                        }
                    }
                    else if (imported != null)
                    {
                        _lastSingleImportedAsset = imported;
                    }
                }


                bool fullRebuild = settings.FullRebuild;
                /* If updating FN. This needs to be done a better way.
                 * Parsing the actual version file would be better.
                 * I'll get to it next release. */
                if (!_ranOnce)
                {
                    _ranOnce = true;
                    fullRebuild = true;
                }
                // Other conditions which a full rebuild may be required.
                else if (!fullRebuild)
                {
                    const string fishnetVersionSave = "fishnet_version";
                    string savedVersion = EditorPrefs.GetString(fishnetVersionSave, string.Empty);
                    if (savedVersion != NetworkManager.FISHNET_VERSION)
                    {
                        fullRebuild = true;
                        EditorPrefs.SetString(fishnetVersionSave, NetworkManager.FISHNET_VERSION);
                    }
                }

                if (fullRebuild)
                    GenerateFull(settings);
                else
                    GenerateChanged(importedAssets, deletedAssets, movedAssets, movedFromAssetPaths, settings);
            }
        }

        /// <summary>
        /// Returns true if a NetworkObject can be added to DefaultPrefabs.
        /// </summary>
        private static bool CanAddNetworkObject(NetworkObject networkObject, PrefabGeneratorConfigurations settings)
        {
            settings = GetSettingsIfNull(settings);
            return networkObject != null && (networkObject.GetIsSpawnable() || !settings.SpawnableOnly);
        }
    }
}

#endif