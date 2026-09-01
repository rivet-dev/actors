#if UNITY_EDITOR
using System;
using FishNet.Configuring;
using UnityEditor;
using UnityEngine;

namespace FishNet.Editing
{
    /// <summary>
    /// Contributed by YarnCat! Thank you!
    /// </summary>
    public class ReviewReminderEditor : EditorWindow
    {
        private Texture2D _fishnetLogo, _reviewButtonBg, _reviewButtonBgHover;
        private GUIStyle _labelStyle, _reviewButtonStyle;
        private const string DATETIME_REMINDED = "ReviewDateTimeReminded";
        private const string CHECK_REMIND_COUNT = "CheckRemindCount";
        private const string IS_ENABLED = "ReminderEnabled";
        private static ReviewReminderEditor _window;

        internal static void CheckRemindToReview()
        {
            bool reminderEnabled = EditorPrefs.GetBool(IS_ENABLED, true);
            if (!reminderEnabled)
                return;

            /* Require at least two opens and 10 days
             * to be passed before reminding. */
            int checkRemindCount = EditorPrefs.GetInt(CHECK_REMIND_COUNT, 0) + 1;
            EditorPrefs.SetInt(CHECK_REMIND_COUNT, checkRemindCount);

            // Not enough checks.
            if (checkRemindCount < 2)
                return;

            string dtStr = EditorPrefs.GetString(DATETIME_REMINDED, string.Empty);
            // Somehow got cleared. Reset.
            if (string.IsNullOrWhiteSpace(dtStr))
            {
                ResetDateTimeReminded();
                return;
            }
            long binary;
            //Failed to parse.
            if (!long.TryParse(dtStr, out binary))
            {
                ResetDateTimeReminded();
                return;
            }
            //Not enough time passed.
            DateTime dt = DateTime.FromBinary(binary);

            if ((DateTime.Now - dt).TotalDays < 10)
                return;

            //If here then the reminder can be shown.
            EditorPrefs.SetInt(CHECK_REMIND_COUNT, 0);
            ResetDateTimeReminded();

            ShowReminder();
        }

        internal static void ResetDateTimeReminded()
        {
            EditorPrefs.SetString(DATETIME_REMINDED, DateTime.Now.ToBinary().ToString());
        }

        private static void ShowReminder()
        {
            InitializeWindow();
        }

        private static void InitializeWindow()
        {
            if (_window != null)
                return;
            _window = (ReviewReminderEditor)GetWindow(typeof(ReviewReminderEditor));
            _window.position = new(0f, 0f, 320f, 300f);
            Rect mainPos;
            mainPos = EditorGUIUtility.GetMainWindowPosition();
            Rect pos = _window.position;
            float w = (mainPos.width - pos.width) * 0.5f;
            float h = (mainPos.height - pos.height) * 0.5f;
            pos.x = mainPos.x + w;
            pos.y = mainPos.y + h;
            _window.position = pos;
        }

        private static void StyleWindow()
        {
            InitializeWindow();
            _window._fishnetLogo = (Texture2D)AssetDatabase.LoadAssetAtPath("Assets/FishNet/Runtime/Editor/Textures/UI/Logo_With_Text.png", typeof(Texture));
            _window._labelStyle = new("label");
            _window._labelStyle.fontSize = 24;
            _window._labelStyle.wordWrap = true;
            //window.labelStyle.alignment = TextAnchor.MiddleCenter;
            _window._labelStyle.normal.textColor = new Color32(74, 195, 255, 255);

            _window._reviewButtonBg = MakeBackgroundTexture(1, 1, new Color32(52, 111, 255, 255));
            _window._reviewButtonBgHover = MakeBackgroundTexture(1, 1, new Color32(99, 153, 255, 255));
            _window._reviewButtonStyle = new("button");
            _window._reviewButtonStyle.fontSize = 18;
            _window._reviewButtonStyle.fontStyle = FontStyle.Bold;
            _window._reviewButtonStyle.normal.background = _window._reviewButtonBg;
            _window._reviewButtonStyle.active.background = _window._reviewButtonBgHover;
            _window._reviewButtonStyle.focused.background = _window._reviewButtonBgHover;
            _window._reviewButtonStyle.onFocused.background = _window._reviewButtonBgHover;
            _window._reviewButtonStyle.hover.background = _window._reviewButtonBgHover;
            _window._reviewButtonStyle.onHover.background = _window._reviewButtonBgHover;
            _window._reviewButtonStyle.alignment = TextAnchor.MiddleCenter;
            _window._reviewButtonStyle.normal.textColor = new(1, 1, 1, 1);
        }

        private void OnGUI()
        {
            float thisWidth = position.width;
            StyleWindow();
            GUILayout.Box(_fishnetLogo, GUILayout.Width(position.width), GUILayout.Height(160f));
            EditorGUILayout.BeginHorizontal();
            GUILayout.Space(8f);
            GUILayout.Label("Have you considered leaving us a review?", _labelStyle, GUILayout.Width(thisWidth * 0.95f));
            EditorGUILayout.EndHorizontal();

            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("Don't Ask Again", GUILayout.Width(position.width)))
            {
                Close();
                EditorPrefs.SetBool(IS_ENABLED, false);
            }
            EditorGUILayout.EndHorizontal();

            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("Ask Later", GUILayout.Width(position.width)))
            {
                Close();
                //Application.OpenURL("https://discord.gg/Ta9HgDh4Hj");
            }
            EditorGUILayout.EndHorizontal();

            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("Leave A Review", GUILayout.Width(position.width)))
            {
                Close();
                EditorPrefs.SetBool(IS_ENABLED, false);
                Application.OpenURL("https://assetstore.unity.com/packages/tools/network/fish-net-networking-evolved-207815");
            }
            EditorGUILayout.EndHorizontal();

            //GUILayout.Space(20);
            //_showOnStartupSelected = EditorGUILayout.Popup("Show on Startup", _showOnStartupSelected, showOnStartupOptions);
        }

        private static Texture2D MakeBackgroundTexture(int width, int height, Color color)
        {
            Color[] pixels = new Color[width * height];
            for (int i = 0; i < pixels.Length; i++)
                pixels[i] = color;
            Texture2D backgroundTexture = new(width, height);
            backgroundTexture.SetPixels(pixels);
            backgroundTexture.Apply();
            return backgroundTexture;
        }
    }
}
#endif