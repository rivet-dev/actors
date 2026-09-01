#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace FishNet.Transporting.Synapse.Editing
{

    [CustomEditor(typeof(Synapse), true)]
    [CanEditMultipleObjects]
    public class SynapseEditor : Editor
    {
        private SerializedProperty _ipv4BindAddress;
        private SerializedProperty _ipv6BindAddress;
        private SerializedProperty _port;
        private SerializedProperty _maximumClients;
        private SerializedProperty _clientAddress;
        private SerializedProperty _synapseConfig;

        protected virtual void OnEnable()
        {
            _ipv4BindAddress = serializedObject.FindProperty(nameof(_ipv4BindAddress));
            _ipv6BindAddress = serializedObject.FindProperty(nameof(_ipv6BindAddress));
            _port = serializedObject.FindProperty(nameof(_port));
            _maximumClients = serializedObject.FindProperty(nameof(_maximumClients));
            _clientAddress = serializedObject.FindProperty(nameof(_clientAddress));
            _synapseConfig = serializedObject.FindProperty(nameof(_synapseConfig));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();
            Synapse synapse = (Synapse)target;

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour(synapse), typeof(Synapse), false);
            GUI.enabled = true;

            EditorGUILayout.LabelField("Server", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_ipv4BindAddress, new GUIContent("IPv4 Bind Address", "IPv4 address to bind the server to. Leave empty to bind to all interfaces."));
            EditorGUILayout.PropertyField(_ipv6BindAddress, new GUIContent("IPv6 Bind Address", "IPv6 address to bind the server to. Leave empty to disable IPv6 binding."));
            EditorGUILayout.PropertyField(_port, new GUIContent("Port", "Port used by both server and client."));
            EditorGUILayout.PropertyField(_maximumClients, new GUIContent("Max Clients", "Maximum number of clients that may be connected at once."));
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            EditorGUILayout.LabelField("Client", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_clientAddress, new GUIContent("Client Address", "Address the client will connect to."));
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            _synapseConfig.isExpanded = EditorGUILayout.Foldout(_synapseConfig.isExpanded, "Configuration", true, EditorStyles.foldoutHeader);

            if (_synapseConfig.isExpanded)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_synapseConfig, GUIContent.none, true);
                EditorGUI.indentLevel--;
            }

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif
