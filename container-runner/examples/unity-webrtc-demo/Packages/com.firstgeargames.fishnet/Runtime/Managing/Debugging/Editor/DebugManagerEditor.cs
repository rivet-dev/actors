#if UNITY_EDITOR
using FishNet.Managing.Debugging;
using GameKit.Dependencies.Utilities;
using UnityEditor;
using UnityEngine;
using LayoutTools = GameKit.Dependencies.Utilities.EditorGuiLayoutTools;

namespace FishNet.Managing.Editing
{
    [CustomEditor(typeof(DebugManager))]
    public class DebugManagerEditor : Editor
    {
        private SerializedProperty _writeSceneObjectDetails;
        private SerializedProperty _validateRpcLengths;
        private SerializedProperty _disableObserversRpcLinks;
        private SerializedProperty _disableTargetRpcLinks;
        private SerializedProperty _disableServerRpcLinks;
        private SerializedProperty _disableReplicateRpcLinks;
        private SerializedProperty _disableReconcileRpcLinks;

        private void OnEnable()
        {
            _writeSceneObjectDetails = serializedObject.FindProperty(nameof(_writeSceneObjectDetails).MemberToPascalCase());
            _validateRpcLengths = serializedObject.FindProperty(nameof(_validateRpcLengths).MemberToPascalCase());
            _disableObserversRpcLinks = serializedObject.FindProperty(nameof(_disableObserversRpcLinks).MemberToPascalCase());
            _disableTargetRpcLinks = serializedObject.FindProperty(nameof(_disableTargetRpcLinks).MemberToPascalCase());
            _disableServerRpcLinks = serializedObject.FindProperty(nameof(_disableServerRpcLinks).MemberToPascalCase());
            _disableReplicateRpcLinks = serializedObject.FindProperty(nameof(_disableReplicateRpcLinks).MemberToPascalCase());
            _disableReconcileRpcLinks = serializedObject.FindProperty(nameof(_disableReconcileRpcLinks).MemberToPascalCase());
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            DebugManager DebugManager = (DebugManager)target;

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour(DebugManager), typeof(DebugManager), false);
            GUI.enabled = true;

            LayoutTools.AddHelpBox("Debug features will only be run in Unity Editor, and development builds. Enabling debug features will increase bandwidth consumption and likely create garbage allocations.", MessageType.Warning);

            EditorGUILayout.Space();
            EditorGUILayout.LabelField("Detail Writing", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            LayoutTools.AddPropertyField(_writeSceneObjectDetails, "Scene Objects");
            EditorGUI.indentLevel--;

            EditorGUILayout.Space();
            EditorGUILayout.LabelField("Packet Validation", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            LayoutTools.AddPropertyField(_validateRpcLengths, "Rpc Lengths");
            EditorGUI.indentLevel--;

            EditorGUILayout.Space();
            EditorGUILayout.LabelField("Disable RpcLinks", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            LayoutTools.AddPropertyField(_disableObserversRpcLinks, "ObserversRpcs");
            LayoutTools.AddPropertyField(_disableTargetRpcLinks, "TargetRpcs");
            LayoutTools.AddPropertyField(_disableServerRpcLinks, "ServerRpcs");
            LayoutTools.AddPropertyField(_disableReplicateRpcLinks, "ReplicateRpcs");
            LayoutTools.AddPropertyField(_disableReconcileRpcLinks, "ReconcileRpcs");
            EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif