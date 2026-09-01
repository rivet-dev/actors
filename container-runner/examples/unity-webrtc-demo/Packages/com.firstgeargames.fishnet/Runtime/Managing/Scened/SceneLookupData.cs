using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using UnityEngine.SceneManagement;

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Extensions for SceneLookupData.
    /// </summary>
    internal static class SceneLookupDataExtensions
    {
        /// <summary>
        /// Returns Names from SceneLookupData.
        /// </summary>
        /// <param name = "datas"></param>
        /// <returns></returns>
        public static string[] GetNames(this SceneLookupData[] datas)
        {
            string[] names = new string[datas.Length];
            for (int i = 0; i < datas.Length; i++)
                names[i] = datas[i].Name;

            return names;
        }

        
        /// <summary>
        /// Returns Names from SceneLookupData.
        /// </summary>
        /// <param name = "datas"></param>
        /// <returns></returns>
        public static string[] GetNamesOnly(this SceneLookupData[] datas)
        {
            string[] names = new string[datas.Length];
            for (int i = 0; i < datas.Length; i++)
                names[i] = datas[i].NameOnly;

            return names;
        }
    }

    /// <summary>
    /// Data container for looking up, loading, or unloading a scene.
    /// </summary>
    public class SceneLookupData : IEquatable<SceneLookupData>
    {
        /// <summary>
        /// Handle of the scene. If value is 0, then handle is not used.
        /// </summary>
        public int Handle;
        /// <summary>
        /// Name of the scene.
        /// </summary>
        public string Name = string.Empty;
        /// <summary>
        /// Returns the scene name without a directory path should one exist.
        /// </summary>
        public string NameOnly
        {
            get
            {
                if (string.IsNullOrEmpty(Name))
                    return string.Empty;
                
                string name = System.IO.Path.GetFileName(Name);
                return RemoveUnityExtension(name);
            }
        }
        /// <summary>
        /// Returns if this data is valid for use.
        /// Being valid does not mean that the scene exist, rather that there is enough data to try and lookup a scene.
        /// </summary>
        public bool IsValid => Name != string.Empty || Handle != 0;

        #region Const
        /// <summary>
        /// String to display when scene data is invalid.
        /// </summary>
        private const string INVALID_SCENE = "One or more scene information entries contain invalid data and have been skipped.";
        #endregion

        /// <summary>
        /// </summary>
        public SceneLookupData() { }

        /// <summary>
        /// </summary>
        /// <param name = "scene">Scene to generate from.</param>
        public SceneLookupData(Scene scene)
        {
            Handle = SceneHandleCompatibility.ToInt(scene);
            Name = scene.name;
        }

        /// <summary>
        /// </summary>
        /// <param name = "name">Scene name to generate from.</param>
        public SceneLookupData(string name)
        {
            Name = name;
        }

        /// <summary>
        /// </summary>
        /// <param name = "handle">Scene handle to generate from.</param>
        public SceneLookupData(int handle)
        {
            Handle = handle;
        }

        /// <summary>
        /// </summary>
        /// <param name = "handle">Scene handle to generate from.</param>
        /// <param name = "name">Name to generate from if handle is 0.</param>
        public SceneLookupData(int handle, string name)
        {
            Handle = handle;
            Name = name;
        }

        #region Comparers.
        public static bool operator ==(SceneLookupData sldA, SceneLookupData sldB)
        {
            // One is null while the other is not.
            if (sldA is null != sldB is null)
                return false;

            /*If here both are either null or have value. */
            if (!(sldA is null))
                return sldA.Equals(sldB);
            else if (!(sldB is null))
                return sldB.Equals(sldA);

            // Fall through indicates both are null.
            return true;
        }

        public static bool operator !=(SceneLookupData sldA, SceneLookupData sldB)
        {
            // One is null while the other is not.
            if (sldA is null != sldB is null)
                return true;

            /*If here both are either null or have value. */
            if (!(sldA is null))
                return !sldA.Equals(sldB);
            else if (!(sldB is null))
                return !sldB.Equals(sldA);

            // Fall through indicates both are null.
            return true;
        }

        public bool Equals(SceneLookupData sld)
        {
            // Comparing instanced against null.
            if (sld is null)
                return false;

            // True if both handles are empty.
            bool bothHandlesEmpty = Handle == 0 && sld.Handle == 0;

            // If both have handles and they match.
            if (!bothHandlesEmpty && sld.Handle == Handle)
                return true;
            // If neither have handles and name matches.
            else if (bothHandlesEmpty && sld.Name == Name)
                return true;

            // Fall through.
            return false;
        }

        public override int GetHashCode()
        {
            int hashCode = 2053068273;
            hashCode = hashCode * -1521134295 + Handle.GetHashCode();
            hashCode = hashCode * -1521134295 + EqualityComparer<string>.Default.GetHashCode(Name);
            return hashCode;
        }

        public override bool Equals(object obj)
        {
            return base.Equals(obj);
        }

        public override string ToString()
        {
            return $"Name {Name}, Handle {Handle}";
            // return base.ToString();
        }
        #endregion

        #region CreateData.
        /// <summary>
        /// Returns a new SceneLookupData.
        /// </summary>
        /// <param name = "scene">Scene to create from.</param>
        /// <returns></returns>
        public static SceneLookupData CreateData(Scene scene) => new(scene);

        /// <summary>
        /// Returns a new SceneLookupData.
        /// </summary>
        /// <param name = "scene">Scene name to create from.</param>
        /// <returns></returns>
        public static SceneLookupData CreateData(string name) => new(name);

        /// <summary>
        /// Returns a new SceneLookupData.
        /// </summary>
        /// <param name = "scene">Scene handle to create from.</param>
        /// <returns></returns>
        public static SceneLookupData CreateData(int handle) => new(handle);

        /// <summary>
        /// Returns a SceneLookupData collection.
        /// </summary>
        /// <param name = "scenes">Scenes to create from.</param>
        /// <returns></returns>
        public static SceneLookupData[] CreateData(List<Scene> scenes) => CreateData(scenes.ToArray());

        /// <summary>
        /// Returns a SceneLookupData collection.
        /// </summary>
        /// <param name = "names">Scene names to create from.</param>
        /// <returns></returns>
        public static SceneLookupData[] CreateData(List<string> names) => CreateData(names.ToArray());

        /// <summary>
        /// Returns a SceneLookupData collection.
        /// </summary>
        /// <param name = "handles">Scene handles to create from.</param>
        /// <returns></returns>
        public static SceneLookupData[] CreateData(List<int> handles) => CreateData(handles.ToArray());

        /// <summary>
        /// Returns a SceneLookupData collection.
        /// </summary>
        /// <param name = "scenes">Scenes to create from.</param>
        /// <returns></returns>
        public static SceneLookupData[] CreateData(Scene[] scenes)
        {
            bool invalidFound = false;
            List<SceneLookupData> result = new();
            foreach (Scene item in scenes)
            {
                if (!item.IsValid())
                {
                    invalidFound = true;
                    continue;
                }

                result.Add(CreateData(item));
            }

            if (invalidFound)
                NetworkManagerExtensions.LogWarning(INVALID_SCENE);

            return result.ToArray();
        }

        /// <summary>
        /// Returns a SceneLookupData collection.
        /// </summary>
        /// <param name = "names">Scene names to create from.</param>
        /// <returns></returns>
        public static SceneLookupData[] CreateData(string[] names)
        {
            SceneLookupData[] result = new SceneLookupData[names.Length];
            for (int i = 0; i < result.Length; i++)
                result[i] = new(names[i]);

            return ValidateData(result);
        }

        /// <summary>
        /// Validates SceneLookupdatas and returns only valid entries.
        /// </summary>
        public static SceneLookupData[] ValidateData(SceneLookupData data) => ValidateData(new SceneLookupData[] { data });

        /// <summary>
        /// Validates SceneLookupdatas and returns only valid entries.
        /// </summary>
        /// <param name = "datas">Datas to validate.</param>
        public static SceneLookupData[] ValidateData(SceneLookupData[] datas)
        {
            bool invalidFound = false;
            List<SceneLookupData> result = CollectionCaches<SceneLookupData>.RetrieveList();
            foreach (SceneLookupData item in datas)
            {
                if (item.IsValid)
                {
                    int failingIndex = -1;
                    // Scene name or handle is set, make sure it's not duplicated in datas.
                    for (int i = 0; i < result.Count; i++)
                    {
                        bool nameMatches = result[i].Name == item.Name;
                        bool handleMatches = result[i].Handle == item.Handle;
                        // Handle is the same (could be 0 handle).
                        if (handleMatches)
                        {
                            // If handle matches and not default then the same scene was added multiple times.
                            if (item.Handle != 0)
                                failingIndex = i;
                        }
                        // Name is the same.
                        else if (nameMatches)
                        {
                            // If handle and name matches then also fail.
                            if (handleMatches)
                                failingIndex = i;
                        }
                    }

                    if (failingIndex != -1)
                        NetworkManagerExtensions.LogWarning($"Data {item.ToString()} matches {result[failingIndex].ToString()} and has been removed from datas.");
                    else
                        result.Add(item);
                }
                else
                {
                    invalidFound = true;
                }
            }

            SceneLookupData[] returnedValue;
            if (invalidFound)
            {
                NetworkManagerExtensions.LogWarning(INVALID_SCENE);
                returnedValue = result.ToArray();
            }
            else
            {
                returnedValue = datas;
            }

            CollectionCaches<SceneLookupData>.Store(result);
            return returnedValue;
        }

        /// <summary>
        /// Returns a SceneLookupData collection.
        /// </summary>
        /// <param name = "handles">Scene handles to create from.</param>
        /// <returns></returns>
        public static SceneLookupData[] CreateData(int[] handles)
        {
            bool invalidFound = false;
            List<SceneLookupData> result = new();
            foreach (int item in handles)
            {
                if (item == 0)
                {
                    invalidFound = true;
                    continue;
                }

                result.Add(CreateData(item));
            }

            if (invalidFound)
                NetworkManagerExtensions.LogWarning(INVALID_SCENE);

            return result.ToArray();
        }
        #endregion

        /// <summary>
        /// Removes .Unity from text.
        /// </summary>
        private static string RemoveUnityExtension(string text)
        {
            string extension = ".unity";
            int extIndex = text.ToLower().IndexOf(extension);
            if (extIndex != -1 && text.Length - extIndex == extension.Length)
                text = text.Substring(0, extIndex);

            return text;
        }

        /// <summary>
        /// Returns the first scene found using Handle or Name, preferring Handle.
        /// </summary>
        /// <returns></returns>
        /// <param name = "foundByHandle">True if scene was found by handle. Handle is always checked first.</param>
        /// <param name = "warn">True to warn if duplicates are found.</param>
        public Scene GetScene(out bool foundByHandle, bool warnIfDuplicates = true)
        {
            foundByHandle = false;

            if (Handle == 0 && string.IsNullOrEmpty(NameOnly))
            {
                NetworkManagerExtensions.LogWarning("Scene handle and name is unset; scene cannot be returned.");
                return default;
            }

            Scene result = default;

            // Lookup my handle.
            if (Handle != 0)
            {
                result = SceneManager.GetScene(Handle);
                if (SceneHandleCompatibility.HasHandle(result))
                    foundByHandle = true;
            }

            // If couldnt find handle try by string.
            if (!foundByHandle)
                result = SceneManager.GetScene(NameOnly, null, warnIfDuplicates);

            return result;
        }
    }
}
