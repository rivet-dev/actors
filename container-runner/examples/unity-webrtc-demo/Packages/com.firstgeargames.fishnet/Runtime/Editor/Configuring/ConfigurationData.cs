#if UNITY_EDITOR
using FishNet.Editing.PrefabCollectionGenerator;
using System;
using System.Collections.Generic;
using System.IO;
using System.Xml.Serialization;
using UnityEditor;
using UnityEngine;

namespace FishNet.Configuring
{
    public enum StrippingTypes : int
    {
        Redirect = 0,
        Empty_Experimental = 1
    }

    public enum SearchScopeType : int
    {
        EntireProject = 0,
        SpecificFolders = 1
    }

    public class CreateNewNetworkBehaviourConfigurations
    {
        public string templateDirectoryPath = "Assets";
    }

    public class PrefabGeneratorConfigurations
    {
        public bool Enabled = true;
        public bool LogToConsole = true;
        public bool FullRebuild = false;
        public bool SpawnableOnly = true;
        public bool SaveChanges = true;
        public string DefaultPrefabObjectsPath = Path.Combine("Assets", "DefaultPrefabObjects.asset");
        internal string DefaultPrefabObjectsPath_Platform => Generator.GetPlatformPath(DefaultPrefabObjectsPath);
        public int SearchScope = (int)SearchScopeType.EntireProject;
        public List<string> ExcludedFolders = new();
        public List<string> IncludedFolders = new();
    }

    public class CodeStrippingConfigurations
    {
        public bool IsBuilding = false;
        public bool IsDevelopment = false;
        public bool IsHeadless = false;
        public bool StripReleaseBuilds = false;
        public int StrippingType = (int)StrippingTypes.Redirect;
    }

    public class ConfigurationData
    {
        // Non serialized doesn't really do anything, its just for me.
        [NonSerialized]
        public bool Loaded;
        public PrefabGeneratorConfigurations PrefabGenerator = new();
        public CodeStrippingConfigurations CodeStripping = new();
        public CreateNewNetworkBehaviourConfigurations CreateNewNetworkBehaviour = new();
    }

    public static class ConfigurationDataExtension
    {
        /// <summary>
        /// Returns if a differs from b.
        /// </summary>
        public static bool HasChanged(this ConfigurationData a, ConfigurationData b)
        {
            return a.CodeStripping.StripReleaseBuilds != b.CodeStripping.StripReleaseBuilds;
        }

        /// <summary>
        /// Copies all values from source to target.
        /// </summary>
        public static void CopyTo(this ConfigurationData source, ConfigurationData target)
        {
            target.CodeStripping.StripReleaseBuilds = source.CodeStripping.StripReleaseBuilds;
        }

        /// <summary>
        /// Writes a configuration data.
        /// </summary>
        public static void Write(this ConfigurationData cd, bool refreshAssetDatabase)
        {
            /* Why is this a thing you ask? Because Unity makes it VERY difficult to read values from
             * memory during builds since on some Unity versions the building application is on a different
             * processor. In result instead of using memory to read configurationdata the values
             * must be written to disk then load the disk values as needed.
             *
             * Fortunatelly the file is extremely small and this does not occur often at all. The disk read
             * will occur once per script save, and once per assembly when building. */
            try
            {
                string path = Configuration.GetAssetsPath(Configuration.CONFIG_FILE_NAME);
                XmlSerializer serializer = new(typeof(ConfigurationData));
                TextWriter writer = new StreamWriter(path);
                serializer.Serialize(writer, cd);
                writer.Close();
#if UNITY_EDITOR
                if (refreshAssetDatabase)
                {
                    AssetDatabase.SaveAssets();
                    AssetDatabase.Refresh();
                }
#endif
            }
            catch (Exception ex)
            {
                throw new($"An error occurred while writing ConfigurationData. Message: {ex.Message}");
            }
        }

        /// <summary>
        /// Writes a configuration data.
        /// </summary>
        public static void Write(this ConfigurationData cd, string path, bool refreshAssetDatabase)
        {
            /* Why is this a thing you ask? Because Unity makes it VERY difficult to read values from
             * memory during builds since on some Unity versions the building application is on a different
             * processor. In result instead of using memory to read configurationdata the values
             * must be written to disk then load the disk values as needed.
             *
             * Fortunatelly the file is extremely small and this does not occur often at all. The disk read
             * will occur once per script save, and once per assembly when building. */
            try
            {
                XmlSerializer serializer = new(typeof(ConfigurationData));
                TextWriter writer = new StreamWriter(path);
                serializer.Serialize(writer, cd);
                writer.Close();
#if UNITY_EDITOR
                if (refreshAssetDatabase)
                {
                    AssetDatabase.SaveAssets();
                    AssetDatabase.Refresh();
                }
#endif
            }
            catch (Exception ex)
            {
                throw new($"An error occurred while writing ConfigurationData. Message: {ex.Message}");
            }
        }
    }
}
#endif