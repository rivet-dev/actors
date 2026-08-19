#if UNITY_EDITOR
using System;
using FishNet.Configuring;
using FishNet.Configuring.EditorCloning;
using FishNet.Managing;
using UnityEditor;
using UnityEngine;

namespace FishNet.Editing
{
    /// <summary>
    /// Contributed by YarnCat! Thank you!
    /// </summary>
    [InitializeOnLoad]
    public class DelayedEditorTasks : EditorWindow
    {
        private static double _startTime = double.MinValue;

        static DelayedEditorTasks()
        {
            if (CloneChecker.IsMultiplayerClone(out _))
                return;

            const string startupCheckString = "FishNetDelayedEditorTasks";
            if (SessionState.GetBool(startupCheckString, false))
                return;

            _startTime = EditorApplication.timeSinceStartup;
            EditorApplication.update += CheckRunTasks;

            SessionState.SetBool(startupCheckString, true);
        }

        private static void CheckRunTasks()
        {
            if (EditorApplication.timeSinceStartup - _startTime < 1f)
                return;

            EditorApplication.update -= CheckRunTasks;

            LogFeedbackLink();

            // First time use, no other actions should be done.
            if (FishNetGettingStartedEditor.ShowGettingStarted())
                return;

            ReviewReminderEditor.CheckRemindToReview();
        }

        private static void LogFeedbackLink()
        {
            // Only log the link when editor opens.
            if (Time.realtimeSinceStartup < 10f)
            {
                string msg = $"Thank you for using Fish-Networking! If you have any feedback -- be suggestions, documentation, or performance related, let us know through our anonymous Google feedback form!{Environment.NewLine}" + @"<color=#67d419>https://forms.gle/1g13VY4KKMnEqpkp6</color>";
                Debug.Log(msg);
            }
        }
    }
}
#endif