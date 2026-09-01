#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace FishNet.Component.Transforming.Beta
{
    [CustomEditor(typeof(OfflineTickSmoother), true)]
    [CanEditMultipleObjects]
    public class OfflineTickSmootherEditor : Editor
    {
        private SerializedProperty _automaticallyInitialize;
        private SerializedProperty _initializationSettings;
        private SerializedProperty _movementSettings;
        private bool _showMovementSettings;

        protected virtual void OnEnable()
        {
            _automaticallyInitialize = serializedObject.FindProperty(nameof(_automaticallyInitialize));
            _initializationSettings = serializedObject.FindProperty(nameof(_initializationSettings));
            _movementSettings = serializedObject.FindProperty(nameof(_movementSettings));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour((OfflineTickSmoother)target), typeof(OfflineTickSmoother), false);
            GUI.enabled = true;

            // EditorGUILayout.LabelField("Initialization Settings", EditorStyles.boldLabel);
            EditorGUILayout.PropertyField(_automaticallyInitialize);
            EditorGUILayout.PropertyField(_initializationSettings);

            _showMovementSettings = EditorGUILayout.Foldout(_showMovementSettings, "Smoothing");
            if (_showMovementSettings)
                EditorGUILayout.PropertyField(_movementSettings);


            // EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif