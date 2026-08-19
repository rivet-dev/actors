#if UNITY_EDITOR
using FishNet.Managing.Object;
using UnityEditor;
using UnityEngine;

namespace FishNet.Managing.Editing
{
    [CustomEditor(typeof(NetworkManager))]
    public class NetworkManagerEditor : Editor
    {
        private SerializedProperty _logging;
        private SerializedProperty _refreshDefaultPrefabs;
        private SerializedProperty _runInBackground;
        private SerializedProperty _dontDestroyOnLoad;
        private SerializedProperty _persistence;
        private SerializedProperty _spawnablePrefabs;
        private SerializedProperty _objectPool;

        private void OnEnable()
        {
            _logging = serializedObject.FindProperty("_logging");
            _refreshDefaultPrefabs = serializedObject.FindProperty("_refreshDefaultPrefabs");
            _runInBackground = serializedObject.FindProperty("_runInBackground");
            _dontDestroyOnLoad = serializedObject.FindProperty("_dontDestroyOnLoad");
            _persistence = serializedObject.FindProperty("_persistence");
            _spawnablePrefabs = serializedObject.FindProperty("_spawnablePrefabs");
            _objectPool = serializedObject.FindProperty("_objectPool");
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            NetworkManager networkManager = (NetworkManager)target;

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour(networkManager), typeof(NetworkManager), false);
            GUI.enabled = true;

            // EditorGUILayout.BeginVertical(GUI.skin.box);
            // EditorGUILayout.EndVertical();


            EditorGUILayout.LabelField("Settings", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_runInBackground);
            EditorGUILayout.PropertyField(_dontDestroyOnLoad);
            EditorGUILayout.PropertyField(_persistence);
            EditorGUILayout.Space();
            EditorGUI.indentLevel--;

            EditorGUILayout.LabelField("Logging", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_logging);
            EditorGUILayout.Space();
            EditorGUI.indentLevel--;

            EditorGUILayout.LabelField("Prefabs", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_spawnablePrefabs);
            EditorGUILayout.PropertyField(_objectPool);
            EditorGUILayout.PropertyField(_refreshDefaultPrefabs);

            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif