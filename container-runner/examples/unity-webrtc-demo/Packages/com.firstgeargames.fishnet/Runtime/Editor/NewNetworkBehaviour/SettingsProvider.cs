#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;
using UnitySettingsProviderAttribute = UnityEditor.SettingsProviderAttribute;
using UnitySettingsProvider = UnityEditor.SettingsProvider;
using FishNet.Configuring;
using System.IO;
using System;
using System.Text.RegularExpressions;

namespace FishNet.Editing.NewNetworkBehaviourScript
{
    internal static class SettingsProvider
    {
        private static CreateNewNetworkBehaviourConfigurations _settings;
        private static GUIContent _folderIcon;
        private static readonly Regex SlashRegex = new(@"[\\// ]");

        [UnitySettingsProvider]
        private static UnitySettingsProvider Create()
        {
            return new("Project/Fish-Networking/New NetworkBehaviour Template", SettingsScope.Project)
            {
                label = "New NetworkBehaviour Template",

                guiHandler = OnGUI,

                keywords = new string[]
                {
                    "Fish",
                    "Networking",
                    "CreateNewNetworkBehaviour",
                    "Template"
                }
            };
        }

        private static void OnGUI(string searchContext)
        {
            if (_settings == null)
                _settings = Configuration.Configurations.CreateNewNetworkBehaviour;

            if (_folderIcon == null)
                _folderIcon = EditorGUIUtility.IconContent("d_FolderOpened Icon");

            EditorGUI.BeginChangeCheck();

            GUILayoutOption iconWidthConstraint = GUILayout.MaxWidth(32.0f);
            GUILayoutOption iconHeightConstraint = GUILayout.MaxHeight(EditorGUIUtility.singleLineHeight);

            if (GUILayout.Button("Edit template"))
            {
                CreateNewNetworkBehaviour.EnsureTemplateExists();

                try
                {
                    System.Diagnostics.Process.Start(CreateNewNetworkBehaviour.TemplatePath);
                }
                catch (Exception e)
                {
                    Debug.LogError($"An issue occurred while trying to launch the NetworkBehaviour template. {e.Message}");
                }
            }
            GUILayout.Space(20);
            EditorGUILayout.BeginHorizontal();
            GUILayout.Label("Template directory: ", GUILayout.MaxWidth(150));
            string newDirectoryPath;

            newDirectoryPath = EditorGUILayout.DelayedTextField(_settings.templateDirectoryPath, GUILayout.MaxWidth(600));
            if (newDirectoryPath.StartsWith("Assets") && Directory.Exists(newDirectoryPath))
            {
                _settings.templateDirectoryPath = newDirectoryPath;
            }
            else
            {
                EditorWindow.focusedWindow.ShowNotification(new($"Directory must be inside the Assets folder."), 2);
            }


            if (GUILayout.Button(_folderIcon, iconHeightConstraint, iconWidthConstraint))
            {
                newDirectoryPath = EditorUtility.OpenFolderPanel("Select template directory", _settings.templateDirectoryPath, "");
            }
            if (newDirectoryPath.StartsWith(Application.dataPath, StringComparison.OrdinalIgnoreCase))
            {
                newDirectoryPath = SlashRegex.Replace(newDirectoryPath.Remove(0, Path.GetDirectoryName(Application.dataPath).Length + 1), Path.DirectorySeparatorChar.ToString());
                _settings.templateDirectoryPath = newDirectoryPath;
            }
            else if (!newDirectoryPath.StartsWith(Application.dataPath, StringComparison.OrdinalIgnoreCase) && !newDirectoryPath.StartsWith("Assets"))
            {
                EditorWindow.focusedWindow.ShowNotification(new($"Directory must be inside the Assets folder."), 2);
            }

            EditorGUILayout.EndHorizontal();

            //            EditorGUILayout.HelpBox("By default MonoBehaviour script template will be copied", MessageType.Info);
            if (EditorGUI.EndChangeCheck())
                Configuration.Configurations.Write(true);
        }
    }
}
#endif