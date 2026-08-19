#if UNITY_EDITOR
using GameKit.Dependencies.Utilities;
using UnityEditor;
using LayoutTools = GameKit.Dependencies.Utilities.EditorGuiLayoutTools;

namespace FishNet.Component.Transforming.Editing
{
    [CustomEditor(typeof(DetachableNetworkTickSmoother), true)]
    [CanEditMultipleObjects]
    public class DetachableNetworkTickSmootherEditor : Editor
    {
        private SerializedProperty _attachOnStop;
        private SerializedProperty _followObject;
        private SerializedProperty _interpolation;
        private SerializedProperty _enableTeleport;
        private SerializedProperty _teleportThreshold;
        private SerializedProperty _synchronizePosition;
        private SerializedProperty _synchronizeRotation;
        private SerializedProperty _synchronizeScale;

        protected virtual void OnEnable()
        {
            _attachOnStop = serializedObject.FindProperty(nameof(_attachOnStop));
            _followObject = serializedObject.FindProperty(nameof(_followObject));
            _interpolation = serializedObject.FindProperty(nameof(_interpolation));
            _enableTeleport = serializedObject.FindProperty(nameof(_enableTeleport));
            _teleportThreshold = serializedObject.FindProperty(nameof(_teleportThreshold));
            _synchronizePosition = serializedObject.FindProperty(nameof(_synchronizePosition));
            _synchronizeRotation = serializedObject.FindProperty(nameof(_synchronizeRotation));
            _synchronizeScale = serializedObject.FindProperty(nameof(_synchronizeScale));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            LayoutTools.AddObjectField("Script:", MonoScript.FromMonoBehaviour((DetachableNetworkTickSmoother)target), typeof(DetachableNetworkTickSmoother), false, EditorLayoutEnableType.Disabled);

            EditorGUILayout.HelpBox("This component will be obsoleted soon. Use NetworkTickSmoother or OfflineTickSmoother.", MessageType.Warning);
            // Misc.
            EditorGUILayout.LabelField("Misc", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_attachOnStop);
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            // Smoothing.
            EditorGUILayout.LabelField("Smoothing", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_followObject);
            EditorGUILayout.PropertyField(_interpolation);

            EditorGUILayout.PropertyField(_enableTeleport);
            if (_enableTeleport.boolValue)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_teleportThreshold);
                EditorGUI.indentLevel--;
            }
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            //Authority.
            EditorGUILayout.LabelField("Synchronizing", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_synchronizePosition);
            EditorGUILayout.PropertyField(_synchronizeRotation);
            EditorGUILayout.PropertyField(_synchronizeScale);
            EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif