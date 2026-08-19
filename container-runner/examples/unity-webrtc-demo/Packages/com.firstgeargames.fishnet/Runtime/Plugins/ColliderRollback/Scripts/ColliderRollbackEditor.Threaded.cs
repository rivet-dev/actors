#if UNITY_EDITOR && FISHNET_THREADED_COLLIDER_ROLLBACK
using UnityEditor;
using UnityEngine;
using static FishNet.Component.ColliderRollback.ColliderRollback;

namespace FishNet.Component.ColliderRollback
{
    [CustomEditor(typeof(ColliderRollback), true)]
    [CanEditMultipleObjects]
    public class ColliderRollbackEditor : Editor
    {
        private SerializedProperty _boundingBox;
        private SerializedProperty _physicsType;
        private SerializedProperty _boundingBoxSize;
        private SerializedProperty _boundingBoxCenter;
        private SerializedProperty _boundingBoxLocalRotation;
        private SerializedProperty _colliderParents;

        protected virtual void OnEnable()
        {
            _boundingBox = serializedObject.FindProperty(nameof(_boundingBox));
            _physicsType = serializedObject.FindProperty(nameof(_physicsType));
            _boundingBoxSize = serializedObject.FindProperty(nameof(_boundingBoxSize));
            _boundingBoxCenter = serializedObject.FindProperty(nameof(_boundingBoxCenter));
            _boundingBoxLocalRotation = serializedObject.FindProperty(nameof(_boundingBoxLocalRotation));
            _colliderParents = serializedObject.FindProperty(nameof(_colliderParents));
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();
            ColliderRollback nob = (ColliderRollback)target;

            GUI.enabled = false;
            EditorGUILayout.ObjectField("Script:", MonoScript.FromMonoBehaviour(nob), typeof(ColliderRollback), false);
            GUI.enabled = true;

            EditorGUILayout.LabelField("Settings", EditorStyles.boldLabel);
            EditorGUI.indentLevel++;

            EditorGUILayout.PropertyField(_boundingBox);
            if ((RollbackManager.BoundingBoxType)_boundingBox.intValue != RollbackManager.BoundingBoxType.Disabled)
            {
                EditorGUI.indentLevel++;
                EditorGUILayout.PropertyField(_physicsType);
                EditorGUILayout.PropertyField(_boundingBoxSize);
                EditorGUILayout.PropertyField(_boundingBoxCenter);
                EditorGUILayout.PropertyField(_boundingBoxLocalRotation);
                EditorGUI.indentLevel--;
            }
            EditorGUILayout.PropertyField(_colliderParents);

            EditorGUI.indentLevel--;

            serializedObject.ApplyModifiedProperties();
        }
        
        private void OnSceneGUI()
        {
            ColliderRollback cr = (ColliderRollback)target;
            RollbackManager.BoundingBoxData bb = cr.GetBoundingBoxData();
            if (bb.boundingBoxType == RollbackManager.BoundingBoxType.Disabled)
                return;

            Transform tr        = cr.transform;
            Vector3 centerWS  = tr.TransformPoint(bb.center);
            Quaternion rotWS     = tr.rotation * bb.localRotation;
            Vector3 lossy     = tr.lossyScale;
            Vector3 absScale  = new Vector3(Mathf.Abs(lossy.x), Mathf.Abs(lossy.y), Mathf.Abs(lossy.z));
            Vector3 sizeWS    = Vector3.Scale((bb.extends * 2f), absScale);

            Matrix4x4 prevMatrix = Handles.matrix;
            Color prevColor  = Handles.color;

            Handles.matrix = Matrix4x4.TRS(centerWS, rotWS, Vector3.one);
            Handles.color  = Color.green;
            Handles.DrawWireCube(Vector3.zero, sizeWS);

            Handles.color  = new Color(1f, 0.6f, 0f, 1f);
            Handles.SphereHandleCap(0, Vector3.zero, Quaternion.identity, 0.03f * Mathf.Max(sizeWS.x, sizeWS.y, sizeWS.z), EventType.Repaint);

            Handles.color  = prevColor;
            Handles.matrix = prevMatrix;
        }
    }
}

#endif