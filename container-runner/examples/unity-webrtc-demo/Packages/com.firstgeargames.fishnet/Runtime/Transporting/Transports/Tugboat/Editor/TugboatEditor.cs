#if UNITY_EDITOR
using FishNet.Object;
using UnityEditor;
using UnityEngine;

namespace FishNet.Transporting.Tugboat.Editing
{
    [CustomEditor(typeof(Tugboat), true)]
    [CanEditMultipleObjects]
    public class TugboatEditor : Editor
    {
        private SerializedProperty _stopSocketsOnThread;
        private SerializedProperty _dontRoute;
        private SerializedProperty _reuseAddress;
        private SerializedProperty _ipv4BindAddress;
        private SerializedProperty _enableIpv6;
        private SerializedProperty _ipv6BindAddress;
        private SerializedProperty _port;
        private SerializedProperty _maximumClients;
        private SerializedProperty _clientAddress;

        protected virtual void OnEnable()
        {
            _stopSocketsOnThread = serializedObject.FindProperty(nameof(_stopSocketsOnThread));
            _dontRoute = serializedObject.FindProperty(nameof(_dontRoute));
            _reuseAddress = serializedObject.FindProperty(nameof(_reuseAddress));
            _ipv4BindAddress = serializedObject.FindProperty(nameof(_ipv4BindAddress));
            _enableIpv6 = serializedObject.FindProperty(nameof(_enableIpv6));
            _ipv6BindAddress = serializedObject.FindProperty(nameof(_ipv6BindAddress));
            _port = serializedObject.FindProperty(nameof(_port));
            _maximumClients = serializedObject.FindProperty(nameof(_maximumClients));
            _clientAddress = serializedObject.FindProperty(nameof(_clientAddress));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();
            Tugboat tb = (Tugboat)target;

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour(tb), typeof(Tugboat), false);
            GUI.enabled = true;

            EditorGUILayout.LabelField("Settings", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_stopSocketsOnThread);
            EditorGUILayout.PropertyField(_dontRoute);
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            EditorGUILayout.LabelField("Server", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_reuseAddress);
            EditorGUILayout.PropertyField(_ipv4BindAddress);
            EditorGUILayout.PropertyField(_enableIpv6);
            if (_enableIpv6.boolValue == true)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_ipv6BindAddress);
                EditorGUI.indentLevel--;
            }
            EditorGUILayout.PropertyField(_port);
            EditorGUILayout.PropertyField(_maximumClients);
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            EditorGUILayout.LabelField("Client", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_clientAddress);
            EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
    }
}

#endif