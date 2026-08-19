#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace FishNet.Managing.Server.Editing
{
    [CustomEditor(typeof(ServerManager), true)]
    [CanEditMultipleObjects]
    public class ServerManagerEditor : Editor
    {
        private SerializedProperty _authenticator;
        private SerializedProperty _remoteClientTimeout;
        private SerializedProperty _remoteClientTimeoutDuration;
        private SerializedProperty _syncTypeRate;
        private SerializedProperty SpawnPacking;
        private SerializedProperty _changeFrameRate;
        private SerializedProperty _frameRate;
        private SerializedProperty _shareIds;
        private SerializedProperty _startOnHeadless;
        private SerializedProperty _allowPredictedSpawning;
        private SerializedProperty _reservedObjectIds;

        protected virtual void OnEnable()
        {
            _authenticator = serializedObject.FindProperty(nameof(_authenticator));
            _remoteClientTimeout = serializedObject.FindProperty(nameof(_remoteClientTimeout));
            _remoteClientTimeoutDuration = serializedObject.FindProperty(nameof(_remoteClientTimeoutDuration));
            _syncTypeRate = serializedObject.FindProperty(nameof(_syncTypeRate));
            SpawnPacking = serializedObject.FindProperty(nameof(SpawnPacking));
            _changeFrameRate = serializedObject.FindProperty(nameof(_changeFrameRate));
            _frameRate = serializedObject.FindProperty(nameof(_frameRate));
            _shareIds = serializedObject.FindProperty(nameof(_shareIds));
            _startOnHeadless = serializedObject.FindProperty(nameof(_startOnHeadless));
            _allowPredictedSpawning = serializedObject.FindProperty(nameof(_allowPredictedSpawning));
            _reservedObjectIds = serializedObject.FindProperty(nameof(_reservedObjectIds));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour((ServerManager)target), typeof(ServerManager), false);
            GUI.enabled = true;

            EditorGUILayout.LabelField("Settings", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_syncTypeRate);
            EditorGUILayout.PropertyField(SpawnPacking);
            EditorGUILayout.PropertyField(_changeFrameRate);
            if (_changeFrameRate.boolValue)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_frameRate);
                EditorGUI.indentLevel--;
            }
            EditorGUILayout.PropertyField(_startOnHeadless);
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            EditorGUILayout.LabelField("Connections", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_remoteClientTimeout);
            if ((RemoteTimeoutType)_remoteClientTimeout.intValue != RemoteTimeoutType.Disabled)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_remoteClientTimeoutDuration, new GUIContent("Timeout"));
                EditorGUI.indentLevel--;
            }
            EditorGUILayout.PropertyField(_shareIds);
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            EditorGUILayout.LabelField("Security", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_authenticator);

            EditorGUILayout.PropertyField(_allowPredictedSpawning);
            if (_allowPredictedSpawning.boolValue == true)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_reservedObjectIds);
                EditorGUI.indentLevel--;
            }
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif