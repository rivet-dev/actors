#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace FishNet.Managing.Observing.Editing
{
    [CustomEditor(typeof(ObserverManager), true)]
    [CanEditMultipleObjects]
    public class ObserverManagerEditor : Editor
    {
        private SerializedProperty _updateHostVisibility;
        private SerializedProperty _maximumTimedObserversDuration;
        private SerializedProperty _defaultConditions
            ;
        private SerializedProperty _useLevelOfDetail;
        private SerializedProperty _maximumLevelOfDetailInterval;
        private SerializedProperty _levelOfDetailUpdateDuration;

        private SerializedProperty _levelOfDetailDistances;

        protected virtual void OnEnable()
        {
            _updateHostVisibility = serializedObject.FindProperty(nameof(_updateHostVisibility));
            _maximumTimedObserversDuration = serializedObject.FindProperty(nameof(_maximumTimedObserversDuration));
            _defaultConditions = serializedObject.FindProperty(nameof(_defaultConditions));

            _useLevelOfDetail = serializedObject.FindProperty(nameof(_useLevelOfDetail));
            _maximumLevelOfDetailInterval = serializedObject.FindProperty(nameof(_maximumLevelOfDetailInterval));
            _levelOfDetailUpdateDuration = serializedObject.FindProperty(nameof(_levelOfDetailUpdateDuration));
            _levelOfDetailDistances = serializedObject.FindProperty(nameof(_levelOfDetailDistances));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour((ObserverManager)target), typeof(ObserverManager), false);
            GUI.enabled = true;

            EditorGUILayout.LabelField("Observers", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;

            EditorGUILayout.PropertyField(_updateHostVisibility);
            if (_maximumTimedObserversDuration.floatValue < 1d)
                EditorGUILayout.HelpBox("Using low values may reduce server performance while under load.", MessageType.Warning);
            EditorGUILayout.PropertyField(_maximumTimedObserversDuration);
            EditorGUILayout.PropertyField(_defaultConditions);

            EditorGUI.indentLevel--;

            EditorGUILayout.LabelField("Level of Detail *", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;

            if (Application.isPlaying)
                GUI.enabled = false;
            
            EditorGUILayout.PropertyField(_useLevelOfDetail);
            if (_useLevelOfDetail.boolValue)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_maximumLevelOfDetailInterval, new GUIContent("Maximum Send Interval"));
                EditorGUILayout.PropertyField(_levelOfDetailUpdateDuration, new GUIContent("Recalculation Duration"));

                EditorGUILayout.PropertyField(_levelOfDetailDistances);
                EditorGUI.indentLevel--;
            }
            
            GUI.enabled = true;
            
            EditorGUI.indentLevel--;
            
            serializedObject.ApplyModifiedProperties();
            }
        }
    }
#endif