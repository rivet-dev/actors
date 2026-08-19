#if UNITY_EDITOR
using UnityEngine;
using UnityEditor;
using FishNet.Configuring;
using UnitySettingsProviderAttribute = UnityEditor.SettingsProviderAttribute;
using UnitySettingsProvider = UnityEditor.SettingsProvider;
using System.Collections.Generic;

namespace FishNet.Configuring.Editing
{
    internal static class SettingsProvider
    {
        private static Vector2 _scrollView;

        [UnitySettingsProvider]
        private static UnitySettingsProvider Create()
        {
            return new("Project/Fish-Networking/Configuration", SettingsScope.Project)
            {
                label = "Configuration",

                guiHandler = OnGUI,

                keywords = new string[]
                {
                    "Fish",
                    "Networking",
                    "Configuration"
                }
            };
        }

        private static void OnGUI(string searchContext)
        {
            ConfigurationData configuration = Configuration.LoadConfigurationData();

            if (configuration == null)
            {
                EditorGUILayout.HelpBox("Unable to load configuration data.", MessageType.Error);

                return;
            }

            EditorGUI.BeginChangeCheck();

            GUIStyle scrollViewStyle = new()
            {
                padding = new(10, 10, 10, 10)
            };

            _scrollView = GUILayout.BeginScrollView(_scrollView, scrollViewStyle);

            EditorGUILayout.BeginHorizontal();

            GUIStyle toggleStyle = new(EditorStyles.toggle)
            {
                richText = true
            };

            configuration.CodeStripping.StripReleaseBuilds = GUILayout.Toggle(configuration.CodeStripping.StripReleaseBuilds, $"{ObjectNames.NicifyVariableName(nameof(configuration.CodeStripping.StripReleaseBuilds))} <color=yellow>(Pro Only)</color>", toggleStyle);

            EditorGUILayout.EndHorizontal();

            if (configuration.CodeStripping.StripReleaseBuilds)
            {
                EditorGUI.indentLevel++;
                // Stripping Method.
                List<string> enumStrings = new();
                foreach (string item in System.Enum.GetNames(typeof(StrippingTypes)))
                    enumStrings.Add(item);
                configuration.CodeStripping.StrippingType = EditorGUILayout.Popup($"{ObjectNames.NicifyVariableName(nameof(configuration.CodeStripping.StrippingType))}", (int)configuration.CodeStripping.StrippingType, enumStrings.ToArray());

                EditorGUILayout.HelpBox("Development builds will not have code stripped. Additionally, if you plan to run as host disable code stripping.", MessageType.Warning);
                EditorGUI.indentLevel--;
            }

            GUILayout.EndScrollView();

            if (EditorGUI.EndChangeCheck())
                Configuration.Configurations.Write(true);
        }
    }
}

#endif