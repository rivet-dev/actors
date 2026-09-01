#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace FishNet.Managing.Timing.Editing
{
    [CustomEditor(typeof(TimeManager), true)]
    [CanEditMultipleObjects]
    public class TimeManagerEditor : Editor
    {
        private SerializedProperty _updateOrder;
        private SerializedProperty _timingType;
        private SerializedProperty _tickRate;
        private SerializedProperty _allowTickDropping;
        private SerializedProperty _maximumFrameTicks;
        private SerializedProperty _pingInterval;
        // private SerializedProperty _timingInterval;
        private SerializedProperty _physicsMode;

        protected virtual void OnEnable()
        {
            _updateOrder = serializedObject.FindProperty("_updateOrder");
            _timingType = serializedObject.FindProperty("_timingType");
            _tickRate = serializedObject.FindProperty("_tickRate");
            _allowTickDropping = serializedObject.FindProperty("_allowTickDropping");
            _maximumFrameTicks = serializedObject.FindProperty("_maximumFrameTicks");
            _pingInterval = serializedObject.FindProperty("_pingInterval");
            // _timingInterval = serializedObject.FindProperty("_timingInterval");
            _physicsMode = serializedObject.FindProperty("_physicsMode");
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour((TimeManager)target), typeof(TimeManager), false);
            GUI.enabled = true;

            // Timing.
            EditorGUILayout.LabelField("Timing", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            EditorGUILayout.PropertyField(_updateOrder);
            EditorGUILayout.PropertyField(_timingType);
            EditorGUILayout.PropertyField(_allowTickDropping);
            if (_allowTickDropping.boolValue == true)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_maximumFrameTicks);
                EditorGUI.indentLevel--;
            }
            EditorGUILayout.PropertyField(_tickRate);
            EditorGUILayout.PropertyField(_pingInterval);
            // EditorGUILayout.PropertyField(_timingInterval);            
            EditorGUI.indentLevel--;
            EditorGUILayout.Space();

            // Physics.
            EditorGUILayout.LabelField("Physics", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;
            if (_physicsMode.intValue == (int)PhysicsMode.TimeManager)
                EditorGUILayout.HelpBox($"Time.fixedDeltaTime will be overridden with TimeManager.TickDelta ({(1f / (float)_tickRate.intValue).ToString("0.###")})", MessageType.Info);
            else
                EditorGUILayout.HelpBox("If you are using physics interactions be sure to change the PhysicsMode to TimeManager and implement physics within the TimeManager tick events. NetworkTransform may also jitter when not using PhysicsMode.TimeManager.", MessageType.Warning);
            EditorGUILayout.PropertyField(_physicsMode);
            EditorGUI.indentLevel--;

            ////Prediction.
            // EditorGUILayout.LabelField("Prediction", EditorStyles.boldLabel);
            // EditorGUI.indentLevel++;
            // EditorGUILayout.PropertyField(_maximumBufferedInputs);
            // EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
    }
}
#endif