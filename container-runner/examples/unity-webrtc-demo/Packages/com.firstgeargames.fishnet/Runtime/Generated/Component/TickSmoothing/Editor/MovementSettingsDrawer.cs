#if UNITY_EDITOR && !THREADED_TICKSMOOTHERS
using FishNet.Object;
using GameKit.Dependencies.Utilities;
using UnityEditor;
using UnityEngine;

namespace FishNet.Component.Transforming.Beta.Editing
{
    [CustomPropertyDrawer(typeof(MovementSettings))]
    public class MovementSettingsDrawer : PropertyDrawer
    {
        private PropertyDrawerTool _propertyDrawer;

        public override void OnGUI(Rect position, SerializedProperty property, GUIContent label)
        {
            EditorGUI.BeginProperty(position, label, property);

            _propertyDrawer = new(position);

            // _propertyDrawer.DrawLabel(label, FontStyle.Bold); 

            EditorGUI.indentLevel++;

            SerializedProperty enableTeleport = property.FindPropertyRelative("EnableTeleport");
            SerializedProperty teleportThreshold = property.FindPropertyRelative("TeleportThreshold");
            SerializedProperty adaptiveInterpolationValue = property.FindPropertyRelative("AdaptiveInterpolationValue");
            SerializedProperty interpolationValue = property.FindPropertyRelative("InterpolationValue");
            SerializedProperty smoothedProperties = property.FindPropertyRelative("SmoothedProperties");
            SerializedProperty snapNonSmoothedProperties = property.FindPropertyRelative("SnapNonSmoothedProperties");

            _propertyDrawer.DrawProperty(enableTeleport, "Enable Teleport");
            if (enableTeleport.boolValue == true)
                _propertyDrawer.DrawProperty(teleportThreshold, "Teleport Threshold", indent: 1);

            _propertyDrawer.DrawProperty(adaptiveInterpolationValue, "Adaptive Interpolation");
            if ((AdaptiveInterpolationType)adaptiveInterpolationValue.intValue == AdaptiveInterpolationType.Off)
                _propertyDrawer.DrawProperty(interpolationValue, "Interpolation Value", indent: 1);

            _propertyDrawer.DrawProperty(smoothedProperties, "Smoothed Properties");
            if ((uint)smoothedProperties.intValue != (uint)TransformPropertiesFlag.Everything)
                _propertyDrawer.DrawProperty(snapNonSmoothedProperties, "Snap Non-Smoothed Properties", indent: 1);

            _propertyDrawer.SetIndentToStarting();

            EditorGUI.EndProperty();
        }

        public override float GetPropertyHeight(SerializedProperty property, GUIContent label) => _propertyDrawer.GetPropertyHeight();
    }
}
#endif