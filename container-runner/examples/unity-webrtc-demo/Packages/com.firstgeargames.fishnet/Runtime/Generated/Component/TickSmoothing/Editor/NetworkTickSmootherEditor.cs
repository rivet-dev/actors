#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace FishNet.Component.Transforming.Beta
{
    [CustomEditor(typeof(NetworkTickSmoother), true)]
    [CanEditMultipleObjects]
    public class NetworkTickSmootherEditor : Editor
    {
        private SerializedProperty _initializationSettings;
        private SerializedProperty _controllerMovementSettings;
        private SerializedProperty _favorPredictionNetworkTransform;
        private SerializedProperty _spectatorMovementSettings;
        private bool _showControllerSmoothingSettings;
        private bool _showSpectatorSmoothingSettings;

        protected virtual void OnEnable()
        {
            _initializationSettings = serializedObject.FindProperty(nameof(_initializationSettings));
            _favorPredictionNetworkTransform = serializedObject.FindProperty(nameof(_favorPredictionNetworkTransform));
            _controllerMovementSettings = serializedObject.FindProperty(nameof(_controllerMovementSettings));
            _spectatorMovementSettings = serializedObject.FindProperty(nameof(_spectatorMovementSettings));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour((NetworkTickSmoother)target), typeof(NetworkTickSmoother), false);
            GUI.enabled = true;

            EditorGUILayout.LabelField("Initialization Settings", EditorStyles.boldLabel);

            EditorGUILayout.PropertyField(_initializationSettings);

            _showControllerSmoothingSettings = EditorGUILayout.Foldout(_showControllerSmoothingSettings, new GUIContent("Controller", "Smoothing applied when object controller. This would be the owner, or if there is no owner and are also server."));
            if (_showControllerSmoothingSettings)
                EditorGUILayout.PropertyField(_controllerMovementSettings);

            _showSpectatorSmoothingSettings = EditorGUILayout.Foldout(_showSpectatorSmoothingSettings, new GUIContent("Spectator Smoothing", "Smoothing applied when object not the owner. This is when server and there is an owner, or when client and not the owner."));
            if (_showSpectatorSmoothingSettings)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_favorPredictionNetworkTransform, new GUIContent("Favor NetworkTransform"));
                EditorGUI.indentLevel--;
                EditorGUILayout.PropertyField(_spectatorMovementSettings);
            }

            // EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif