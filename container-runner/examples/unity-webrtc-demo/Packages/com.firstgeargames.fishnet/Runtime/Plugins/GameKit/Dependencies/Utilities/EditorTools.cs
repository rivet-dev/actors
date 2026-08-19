#if UNITY_EDITOR
using System;
using UnityEditor;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public enum EditorLayoutEnableType
    {
        Enabled = 0,
        Disabled = 1,
        DisabledWhilePlaying = 2
    }

    public static class EditorGuiLayoutTools
    {
        /// <summary>
        /// Adds a helpbox field.
        /// </summary>
        public static void AddHelpBox(string text, MessageType messageType = MessageType.Info)
        {
            EditorGUILayout.HelpBox(text, messageType);
        }

        /// <summary>
        /// Adds a property field.
        /// </summary>
        public static void AddPropertyField(SerializedProperty sp, string fieldName, string tooltip = "")
        {
            if (tooltip == "")
                tooltip = sp.tooltip;

            EditorGUILayout.PropertyField(sp, new GUIContent(fieldName, tooltip));
        }

        /// <summary>
        /// Adds a property field.
        /// </summary>
        public static void AddPropertyField(SerializedProperty sp, GUIContent guiContent)
        {
            EditorGUILayout.PropertyField(sp, guiContent);
        }

        /// <summary>
        /// Adds a property field.
        /// </summary>
        public static void AddPropertyField(SerializedProperty sp, GUIContent guiContent = null, EditorLayoutEnableType enableType = EditorLayoutEnableType.Enabled, params GUILayoutOption[] options)
        {
            bool disable = IsDisableLayoutType(enableType);
            if (disable)
                GUI.enabled = false;

            EditorGUILayout.PropertyField(sp, guiContent, options);

            if (disable)
                GUI.enabled = true;
        }

        /// <summary>
        /// Adds a property field.
        /// </summary>
        /// <param name = "enabled">True to have property enabled.</param>
        [Obsolete("Use AddPropertyField(SerializedProperty, GUIContent, EditorLayoutEnableType, GUILayoutOption.")]
        public static void AddPropertyField(SerializedProperty sp, GUIContent guiContent = null, bool enabled = true, params GUILayoutOption[] options)
        {
            EditorLayoutEnableType enableType = enabled ? EditorLayoutEnableType.Enabled : EditorLayoutEnableType.Disabled;
            bool disable = IsDisableLayoutType(enableType);
            if (disable)
                GUI.enabled = false;

            EditorGUILayout.PropertyField(sp, guiContent, options);

            if (disable)
                GUI.enabled = true;
        }

        /// <summary>
        /// Adds an object field.
        /// </summary>
        public static void AddObjectField(string label, MonoScript ms, Type type, bool allowSceneObjects, EditorLayoutEnableType enableType = EditorLayoutEnableType.Enabled, params GUILayoutOption[] options)
        {
            bool disable = IsDisableLayoutType(enableType);
            if (disable)
                GUI.enabled = false;

            EditorGUILayout.ObjectField("Script:", ms, type, allowSceneObjects, options);

            if (disable)
                GUI.enabled = true;
        }

        /// <summary>
        /// Disables GUI if playing.
        /// </summary>
        public static void DisableGUIIfPlaying()
        {
            if (Application.isPlaying)
                GUI.enabled = false;
        }

        /// <summary>
        /// Enables GUI if playing.
        /// </summary>
        public static void EnableGUIIfPlaying()
        {
            if (Application.isPlaying)
                GUI.enabled = true;
        }

        /// <summary>
        /// Returns if a layout field should be disabled.
        /// </summary>
        /// <param name = "enableType"></param>
        /// <returns></returns>
        private static bool IsDisableLayoutType(EditorLayoutEnableType enableType)
        {
            return enableType == EditorLayoutEnableType.Disabled || (enableType == EditorLayoutEnableType.DisabledWhilePlaying && Application.isPlaying);
        }
    }

    public static class PropertyDrawerToolExtensions
    {
        /// <summary>
        /// Returns GetPropertyHeight value based on drawerTool properties.
        /// </summary>
        public static float GetPropertyHeight(this PropertyDrawerTool drawerTool)
        {
            if (drawerTool == null)
                return EditorGUIUtility.singleLineHeight;

            return EditorGUIUtility.singleLineHeight * drawerTool.LineSpacingMultiplier * drawerTool.PropertiesDrawn;
        }
    }

    /// <summary>
    /// Various utility classes relating to floats.
    /// </summary>
    public class PropertyDrawerTool
    {
        public PropertyDrawerTool()
        {
            Debug.LogError($"This initializer is not supported. Use the initializer with arguments.");
        }

        public PropertyDrawerTool(Rect position, float lineSpacingMultiplier = 1f)
        {
            Position = position;
            LineSpacingMultiplier = lineSpacingMultiplier;
            Position = position;
            _startingIndent = EditorGUI.indentLevel;
        }

        /// <summary>
        /// Starting position as indicated by the OnGUI method.
        /// </summary>
        /// <remarks>This value may be modified by user code.</remarks>
        public Rect Position = default;
        /// <summary>
        /// Preferred spacing between each draw.
        /// </summary>
        public float LineSpacingMultiplier;
        /// <summary>
        /// Number of entries drawn by this object.
        /// </summary>
        public int PropertiesDrawn = 0;
        /// <summary>
        /// Additional position Y of next draw.
        /// </summary>
        private float _additionalPositionY = 0;
        /// <summary>
        /// Indent level during initialization.
        /// </summary>
        private readonly int _startingIndent;

        /// <summary>
        /// Sets EditorGUI.Indent to the level it were when initializing this class.
        /// </summary>
        public void SetIndentToStarting() => EditorGUI.indentLevel = _startingIndent;

        /// <summary>
        /// Draws a label.
        /// </summary>
        public void DrawLabel(GUIContent lLabel) => DrawLabel(lLabel, EditorStyles.label.fontStyle, indent: 0);

        /// <summary>
        /// Draws a label.
        /// </summary>
        public void DrawLabel(GUIContent lLabel, FontStyle styleOverride) => DrawLabel(lLabel, styleOverride, indent: 0);

        /// <summary>
        /// Draws a label.
        /// </summary>
        public void DrawLabel(GUIContent lLabel, FontStyle styleOverride, int indent)
        {
            PropertiesDrawn++;

            if (indent != 0)
                EditorGUI.indentLevel += indent;

            // Set style.
            FontStyle startingStyle = EditorStyles.label.fontStyle;
            EditorStyles.label.fontStyle = styleOverride;

            EditorGUI.PrefixLabel(GetRect(), GUIUtility.GetControlID(FocusType.Passive), lLabel);

            EditorStyles.label.fontStyle = startingStyle;

            if (indent != 0)
                EditorGUI.indentLevel -= indent;
        }

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop) => DrawProperty(prop, lLabel: "", indent: 0);

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, string label) => DrawProperty(prop, new GUIContent(label), EditorStyles.label.fontStyle, indent: 0);

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, GUIContent content) => DrawProperty(prop, content, EditorStyles.label.fontStyle, indent: 0);

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, int indent) => DrawProperty(prop, lLabel: "", indent);

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, string lLabel, int indent) => DrawProperty(prop, lLabel, EditorStyles.label.fontStyle, indent);

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, GUIContent content, int indent) => DrawProperty(prop, content, EditorStyles.label.fontStyle, indent);

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, GUIContent content, FontStyle labelStyle) => DrawProperty(prop, content, labelStyle, indent: 0);

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, string lLabel, FontStyle labelStyle, int indent)
        {
            GUIContent content = lLabel == "" ? default : new GUIContent(lLabel);

            DrawProperty(prop, content, labelStyle, indent);
        }

        /// <summary>
        /// Draws a property.
        /// </summary>
        public void DrawProperty(SerializedProperty prop, GUIContent content, FontStyle labelStyle, int indent)
        {
            PropertiesDrawn++;

            EditorGUI.indentLevel += indent;

            FontStyle startingStyle = EditorStyles.label.fontStyle;
            EditorStyles.label.fontStyle = labelStyle;

            EditorGUI.PropertyField(GetRect(), prop, content);

            EditorStyles.label.fontStyle = startingStyle;

            EditorGUI.indentLevel -= indent;
        }
        
        /// <summary>
        /// Draws a help box.
        /// </summary>
        public void DrawHelpBox(string message, MessageType type = MessageType.Info, int indent = 0)
        {
            PropertiesDrawn++;

            if (indent != 0)
                EditorGUI.indentLevel += indent;

            // Calculate how much height the help box needs based on the current width
            // We subtract the indent spacing from the width to ensure text wrapping is calculated correctly
            float indentSpacing = indent * 15f; 
            float height = EditorStyles.helpBox.CalcHeight(new(message), Position.width - indentSpacing);

            // Get the rect and draw
            EditorGUI.HelpBox(GetRectForHelpBox(height), message, type);

            if (indent != 0)
                EditorGUI.indentLevel -= indent;
        }

        /// <summary>
        /// Specialized Rect getter for elements with variable heights like HelpBoxes.
        /// </summary>
        private Rect GetRectForHelpBox(float height)
        {
            Rect result = new(Position.x, Position.y + _additionalPositionY, Position.width, height);

            // Advance the Y position by the specific height of the box + standard spacing
            _additionalPositionY += height + EditorGUIUtility.standardVerticalSpacing;

            return result;
        }

        /// <summary>
        /// Gets the next Rect to draw at.
        /// </summary>
        /// <returns></returns>
        public Rect GetRect(float? lineSpacingMultiplierOverride = null)
        {
            float multiplier = lineSpacingMultiplierOverride ?? LineSpacingMultiplier;

            Rect result = new(Position.x, Position.y + _additionalPositionY, Position.width, EditorGUIUtility.singleLineHeight * multiplier);

            _additionalPositionY += EditorGUIUtility.singleLineHeight * multiplier;

            return result;
        }
    }
}
#endif