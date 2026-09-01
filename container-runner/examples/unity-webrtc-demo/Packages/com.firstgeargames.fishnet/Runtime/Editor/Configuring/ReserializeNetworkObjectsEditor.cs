#if UNITY_EDITOR
using System.Collections.Generic;
using System.Linq;
using FishNet.Editing.PrefabCollectionGenerator;
using FishNet.Object;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityScene = UnityEngine.SceneManagement.Scene;
using UnitySceneManagement = UnityEngine.SceneManagement;

namespace FishNet.Editing
{
    /// <summary>
    /// Contributed by YarnCat! Thank you!
    /// </summary>
    public class ReserializeNetworkObjectsEditor : EditorWindow
    {
        /// <summary>
        /// True if currently iterating.
        /// </summary>
        [System.NonSerialized]
        internal static bool IsRunning;

        private enum ReserializeSceneType : int
        {
            AllScenes = 0,
            OpenScenes = 1,
            SelectedScenes = 2,
            BuildScenes = 3
        }

        private struct OpenScene
        {
            public UnityScene Scene;
            public string Path;

            public OpenScene(UnityScene scene)
            {
                Scene = scene;
                Path = scene.path;
            }
        }

        private Texture2D _fishnetLogo;
        private Texture2D _buttonBg;
        private Texture2D _buttonBgHover;
        private GUIStyle _upgradeRequiredStyle;
        private GUIStyle _instructionsStyle;
        private GUIStyle _buttonStyle;
        private bool _loaded;
        private bool _iteratePrefabs;
        private bool _iterateScenes;
        private ReserializeSceneType _sceneReserializeType = ReserializeSceneType.OpenScenes;
        private bool _enabledOnlyBuildScenes = true;
        private const string UPGRADE_PART_COLOR = "cd61ff";
        private const string UPGRADE_COMPLETE_COLOR = "32e66e";
        private const string PREFS_PREFIX = "FishNetReserialize";
        private static ReserializeNetworkObjectsEditor _window;

        [MenuItem("Tools/Fish-Networking/Utility/Reserialize NetworkObjects", false, 400)]
        internal static void ReserializeNetworkObjects()
        {
            if (ApplicationState.IsPlaying())
            {
                Debug.LogError($"NetworkObjects cannot be reserialized while in play mode.");
                return;
            }

            InitializeWindow();
        }

        private static void InitializeWindow()
        {
            if (_window != null)
                return;

            _window = (ReserializeNetworkObjectsEditor)GetWindow(typeof(ReserializeNetworkObjectsEditor));
            _window.position = new(0f, 0f, 550f, 300f);
            Rect mainPos;
            mainPos = EditorGUIUtility.GetMainWindowPosition();
            Rect pos = _window.position;
            float w = (mainPos.width - pos.width) * 0.5f;
            float h = (mainPos.height - pos.height) * 0.5f;
            pos.x = mainPos.x + w;
            pos.y = mainPos.y + h;
            _window.position = pos;
        }

        private static void StyleWindow()
        {
            if (_window == null)
                return;

            _window._fishnetLogo = (Texture2D)AssetDatabase.LoadAssetAtPath("Assets/FishNet/Runtime/Editor/Textures/UI/Logo_With_Text.png", typeof(Texture));
            _window._upgradeRequiredStyle = new("label");
            _window._upgradeRequiredStyle.fontSize = 20;
            _window._upgradeRequiredStyle.wordWrap = true;
            _window._upgradeRequiredStyle.alignment = TextAnchor.MiddleCenter;
            _window._upgradeRequiredStyle.normal.textColor = new Color32(255, 102, 102, 255);

            _window._instructionsStyle = new("label");
            _window._instructionsStyle.fontSize = 14;
            _window._instructionsStyle.wordWrap = true;
            _window._instructionsStyle.alignment = TextAnchor.MiddleCenter;
            _window._instructionsStyle.normal.textColor = new Color32(255, 255, 255, 255);
            _window._instructionsStyle.hover.textColor = new Color32(255, 255, 255, 255);

            _window._buttonBg = MakeBackgroundTexture(1, 1, new Color32(52, 111, 255, 255));
            _window._buttonBgHover = MakeBackgroundTexture(1, 1, new Color32(99, 153, 255, 255));
            _window._buttonStyle = new("button");
            _window._buttonStyle.fontSize = 18;
            _window._buttonStyle.fontStyle = FontStyle.Bold;
            _window._buttonStyle.normal.background = _window._buttonBg;
            _window._buttonStyle.active.background = _window._buttonBgHover;
            _window._buttonStyle.focused.background = _window._buttonBgHover;
            _window._buttonStyle.onFocused.background = _window._buttonBgHover;
            _window._buttonStyle.hover.background = _window._buttonBgHover;
            _window._buttonStyle.onHover.background = _window._buttonBgHover;
            _window._buttonStyle.alignment = TextAnchor.MiddleCenter;
            _window._buttonStyle.normal.textColor = new(1, 1, 1, 1);
        }

        private void OnGUI()
        {
            // If not yet loaded then set last used values.
            if (!_loaded)
            {
                LoadLastValues();
                _loaded = true;
            }

            float thisWidth = position.width;
            StyleWindow();
            // Starting values.
            Vector2 requiredSize = new(position.width, 160f);

            GUILayout.Box(_fishnetLogo, GUILayout.Width(requiredSize.x), GUILayout.Height(requiredSize.y));

            GUILayout.Space(8f);
            EditorGUILayout.BeginHorizontal();
            GUILayout.Space(5f);
            CreateInformationLabel("Use this window to refresh serialized values on all NetworkObject prefabs and scene NetworkObjects.");
            EditorGUILayout.EndHorizontal();
            GUILayout.Space(8f);

            EditorGUILayout.BeginHorizontal();
            GUILayout.Space(30f);
            _iteratePrefabs = EditorGUILayout.Toggle("Reserialize Prefabs", _iteratePrefabs);
            EditorGUILayout.EndHorizontal();

            EditorGUILayout.BeginHorizontal();
            // Some dumb reason Unity moves the checkbox further when using nested settings.
            float rebuildScenesSpacing = _iterateScenes ? 27f : 30f;
            GUILayout.Space(rebuildScenesSpacing);
            EditorGUILayout.BeginVertical();

            _iterateScenes = EditorGUILayout.Toggle("Reserialize Scenes", _iterateScenes);

            if (_iterateScenes)
            {
                EditorGUILayout.BeginHorizontal();
                GUILayout.Space(15f);
                _sceneReserializeType = (ReserializeSceneType)EditorGUILayout.EnumPopup("Targeted Scenes", _sceneReserializeType);
                EditorGUILayout.EndHorizontal();

                requiredSize.y += 20f;

                if (_sceneReserializeType == ReserializeSceneType.BuildScenes)
                {
                    EditorGUILayout.BeginHorizontal();
                    GUILayout.Space(30f);
                    _enabledOnlyBuildScenes = EditorGUILayout.Toggle("Enabled Only", _enabledOnlyBuildScenes);
                    EditorGUILayout.EndHorizontal();
                    requiredSize.y += 18f;
                }

                if (_sceneReserializeType != ReserializeSceneType.OpenScenes)
                {
                    EditorGUILayout.BeginHorizontal();
                    GUILayout.Space(30f);
                    EditorGUILayout.HelpBox("This operation will open and close targeted scene one at a time. Your current open scenes will be closed and re-opened without saving.", MessageType.Warning);
                    EditorGUILayout.EndHorizontal();
                    requiredSize.y += 40f;
                }
            }

            EditorGUILayout.EndVertical();
            EditorGUILayout.EndHorizontal();

            requiredSize.y += 80f;

            GUILayout.Space(8f);

            EditorGUILayout.BeginHorizontal();

            if (!_iteratePrefabs && !_iterateScenes)
                GUI.enabled = false;

            if (GUILayout.Button("Run Task"))
            {
                IsRunning = true;

                SaveLastValues();

                ReserializeProjectPrefabs();
                ReserializeScenes();

                LogColoredText($"Task complete.", UPGRADE_COMPLETE_COLOR);

                _iteratePrefabs = false;
                _iterateScenes = false;

                IsRunning = false;
            }

            GUI.enabled = true;
            EditorGUILayout.EndHorizontal();

            minSize = requiredSize;
            maxSize = minSize;

            void CreateInformationLabel(string text, FontStyle? style = null)
            {
                EditorGUILayout.BeginHorizontal();

                FontStyle firstStyle = _instructionsStyle.fontStyle;
                if (style != null)
                    _instructionsStyle.fontStyle = style.Value;

                GUILayout.Label(text, _instructionsStyle, GUILayout.Width(thisWidth * 0.95f));

                _instructionsStyle.fontStyle = firstStyle;
                EditorGUILayout.EndHorizontal();

                requiredSize.y += 55f;
            }
        }

        private void LoadLastValues()
        {
            _iteratePrefabs = EditorPrefs.GetBool($"{PREFS_PREFIX}{nameof(_iteratePrefabs)}", defaultValue: false);
            _iterateScenes = EditorPrefs.GetBool($"{PREFS_PREFIX}{nameof(_iterateScenes)}", defaultValue: false);
            _sceneReserializeType = (ReserializeSceneType)EditorPrefs.GetInt($"{PREFS_PREFIX}{nameof(_sceneReserializeType)}", defaultValue: (int)ReserializeSceneType.OpenScenes);
            _enabledOnlyBuildScenes = EditorPrefs.GetBool($"{PREFS_PREFIX}{nameof(_enabledOnlyBuildScenes)}", defaultValue: true);
        }

        private void SaveLastValues()
        {
            EditorPrefs.SetBool($"{PREFS_PREFIX}{nameof(_iteratePrefabs)}", _iteratePrefabs);
            EditorPrefs.SetBool($"{PREFS_PREFIX}{nameof(_iterateScenes)}", _iterateScenes);
            EditorPrefs.SetInt($"{PREFS_PREFIX}{nameof(_sceneReserializeType)}", (int)_sceneReserializeType);
            EditorPrefs.SetBool($"{PREFS_PREFIX}{nameof(_enabledOnlyBuildScenes)}", _enabledOnlyBuildScenes);
        }

        private void ReserializeProjectPrefabs()
        {
            if (!_iteratePrefabs)
                return;

            int checkedObjects = 0;
            int duplicateNetworkObjectsRemoved = 0;

            bool modified = false;

            List<NetworkObject> networkObjects = Generator.GetNetworkObjects(settings: null);
            foreach (NetworkObject nob in networkObjects)
            {
                checkedObjects++;
                duplicateNetworkObjectsRemoved += nob.RemoveDuplicateNetworkObjects();

                nob.ReserializeEditorSetValues(setWasActiveDuringEdit: true, setSceneId: false);
                EditorUtility.SetDirty(nob);

                modified = true;
            }

            if (modified)
                AssetDatabase.SaveAssets();

            Debug.Log($"Reserialized {checkedObjects} NetworkObject prefabs. Removed {duplicateNetworkObjectsRemoved} duplicate NetworkObject components.");
        }

        private void ReserializeScenes()
        {
            if (!_iterateScenes)
                return;

            int duplicateNetworkObjectsRemoved = 0;
            int checkedObjects = 0;
            int checkedScenes = 0;
            int changedObjects = 0;

            List<OpenScene> openScenes = GetOpenScenes();

            // If running for open scenes only.
            if (_sceneReserializeType == ReserializeSceneType.OpenScenes)
            {
                ReserializeScenes(openScenes, ref checkedScenes, ref checkedObjects, ref changedObjects, ref duplicateNetworkObjectsRemoved);
            }
            // Running on multiple scenes.
            else
            {
                // When working on multiple scenes make sure open scenes are not dirty to prevent data loss.
                foreach (OpenScene os in openScenes)
                {
                    if (os.Scene.isDirty)
                    {
                        Debug.LogError($"One or more open scenes are dirty. To prevent data loss scene reserialization will not complete. Ensure all open scenes are saved before continuing.");
                        return;
                    }
                }
                List<SceneAsset> targetedScenes;
                if (_sceneReserializeType == ReserializeSceneType.SelectedScenes)
                {
                    targetedScenes = Selection.GetFiltered<SceneAsset>(SelectionMode.Assets).ToList();
                }
                else if (_sceneReserializeType == ReserializeSceneType.AllScenes)
                {
                    targetedScenes = new();

                    string[] scenePaths = Generator.GetProjectFiles("Assets", "unity", new(), recursive: true);
                    foreach (string path in scenePaths)
                    {
                        SceneAsset sceneAsset = AssetDatabase.LoadAssetAtPath<SceneAsset>(path);
                        if (sceneAsset != null)
                            targetedScenes.Add(sceneAsset);
                    }
                }
                else if (_sceneReserializeType == ReserializeSceneType.BuildScenes)
                {
                    targetedScenes = new();

                    EditorBuildSettingsScene[] buildScenes = EditorBuildSettings.scenes;
                    foreach (EditorBuildSettingsScene bs in buildScenes)
                    {
                        if (_enabledOnlyBuildScenes && !bs.enabled)
                            continue;

                        SceneAsset sceneAsset = AssetDatabase.LoadAssetAtPath<SceneAsset>(bs.path);
                        if (sceneAsset != null)
                            targetedScenes.Add(sceneAsset);
                    }
                }
                else
                {
                    Debug.LogError($"Unsupported {nameof(ReserializeSceneType)} type {_sceneReserializeType}.");
                    return;
                }

                ReserializeScenes(targetedScenes, ref checkedScenes, ref checkedObjects, ref changedObjects, ref duplicateNetworkObjectsRemoved);

                // Reopen original scenes.
                for (int i = 0; i < openScenes.Count; i++)
                {
                    string path = openScenes[i].Path;

                    /* Make sure asset exists before trying to reopen scene.
                     * Its possible the dev had a scene open that wasn't saved, which
                     * would otherwise result in an error here. */
                    SceneAsset sceneAsset = AssetDatabase.LoadAssetAtPath<SceneAsset>(path);
                    if (sceneAsset != null)
                    {
                        OpenSceneMode mode = i == 0 ? OpenSceneMode.Single : OpenSceneMode.Additive;
                        EditorSceneManager.OpenScene(path, mode);
                    }
                }
            }

            if (changedObjects > 0)
                AssetDatabase.SaveAssets();

            string saveText = _sceneReserializeType == ReserializeSceneType.OpenScenes && changedObjects > 0 ? " Please save your open scenes." : string.Empty;
            Debug.Log($"Checked {checkedObjects} NetworkObjects over {checkedScenes} scenes. {changedObjects} sceneIds were generated. {duplicateNetworkObjectsRemoved} duplicate NetworkObject components were removed. {saveText}");

            LogColoredText($"Scene NetworkObjects refreshed.", UPGRADE_PART_COLOR);

            List<OpenScene> GetOpenScenes()
            {
                List<OpenScene> result = new();
                int sceneCount = UnitySceneManagement.SceneManager.sceneCount;
                for (int i = 0; i < sceneCount; i++)
                {
                    UnityScene scene = UnitySceneManagement.SceneManager.GetSceneAt(i);
                    if (scene.isLoaded)
                        result.Add(new(scene));
                }

                return result;
            }
        }

        /// <summary>
        /// Refreshes NetworkObjects for specified scenes.
        /// </summary>
        private static void ReserializeScenes(List<SceneAsset> sceneAssets, ref int checkedScenes, ref int checkedObjects, ref int changedObjects, ref int duplicateNetworkObjectsRemoved)
        {
            foreach (SceneAsset sa in sceneAssets)
            {
                string path = AssetDatabase.GetAssetPath(sa);
                UnityScene scene = EditorSceneManager.OpenScene(path, OpenSceneMode.Single);
                List<NetworkObject> foundNobs = NetworkObject.CreateSceneId(scene, force: true, out int changed);

                foreach (NetworkObject n in foundNobs)
                {
                    duplicateNetworkObjectsRemoved += n.RemoveDuplicateNetworkObjects();
                    n.ReserializeEditorSetValues(setWasActiveDuringEdit: true, setSceneId: false);
                }

                EditorSceneManager.SaveScene(scene);

                checkedScenes++;
                checkedObjects += foundNobs.Count;
                changedObjects += changed;
            }
        }

        /// <summary>
        /// Refreshes NetworkObjects in OpenScenes.
        /// </summary>
        private static void ReserializeScenes(List<OpenScene> openScenes, ref int checkedScenes, ref int checkedObjects, ref int changedObjects, ref int duplicateNetworkObjectsRemoved)
        {
            foreach (OpenScene os in openScenes)
            {
                List<NetworkObject> foundNobs = NetworkObject.CreateSceneId(os.Scene, force: true, out int changed);

                foreach (NetworkObject n in foundNobs)
                {
                    duplicateNetworkObjectsRemoved += n.RemoveDuplicateNetworkObjects();
                    n.ReserializeEditorSetValues(setWasActiveDuringEdit: true, setSceneId: false);
                }

                checkedScenes++;
                checkedObjects += foundNobs.Count;
                changedObjects += changed;
            }
        }

        private static void LogColoredText(string txt, string hexColor)
        {
            Debug.Log($"<color=#{hexColor}>{txt}</color>");
        }

        private static Texture2D MakeBackgroundTexture(int width, int height, Color color)
        {
            Color[] pixels = new Color[width * height];
            for (int i = 0; i < pixels.Length; i++)
                pixels[i] = color;
            Texture2D backgroundTexture = new(width, height);
            backgroundTexture.SetPixels(pixels);
            backgroundTexture.Apply();
            return backgroundTexture;
        }
    }
}
#endif