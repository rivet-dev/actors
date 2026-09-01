#if UNITY_EDITOR
using GameKit.Dependencies.Utilities;
using UnityEditor;

namespace FishNet.Managing.Logging.Editing
{
    [CustomEditor(typeof(LevelLoggingConfiguration), true)]
    [CanEditMultipleObjects]
    public class LevelLoggingConfigurationEditor : Editor
    {
        private SerializedProperty _isEnabled;
        private SerializedProperty _addLocalTick;
        private SerializedProperty _addTimestamps;
        private SerializedProperty _enableTimestampsInEditor;
        private SerializedProperty _developmentLogging;
        private SerializedProperty _guiLogging;
        private SerializedProperty _headlessLogging;

        protected virtual void OnEnable()
        {
            _isEnabled = serializedObject.FindProperty(nameof(_isEnabled).MemberToPascalCase());

            _addLocalTick = serializedObject.FindProperty(nameof(_addLocalTick));

            _addTimestamps = serializedObject.FindProperty(nameof(_addTimestamps));
            _enableTimestampsInEditor = serializedObject.FindProperty(nameof(_enableTimestampsInEditor));

            _developmentLogging = serializedObject.FindProperty(nameof(_developmentLogging));
            _guiLogging = serializedObject.FindProperty(nameof(_guiLogging));
            _headlessLogging = serializedObject.FindProperty(nameof(_headlessLogging));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            EditorGUILayout.PropertyField(_isEnabled);

            if (_isEnabled.boolValue == false)
                return;

            EditorGUI.indentLevel++;

            EditorGUILayout.PropertyField(_addLocalTick);

            EditorGUILayout.PropertyField(_addTimestamps);
            if (_addTimestamps.boolValue == true)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_enableTimestampsInEditor);
                EditorGUI.indentLevel--;
            }

            EditorGUILayout.PropertyField(_developmentLogging);
            EditorGUILayout.PropertyField(_guiLogging);
            EditorGUILayout.PropertyField(_headlessLogging);

            EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
    }
}

#endif