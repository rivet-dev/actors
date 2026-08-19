#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Documenting;
using System;
using System.Runtime.CompilerServices;
using System.Text;
using FishNet.Managing.Timing;
using UnityEngine;

namespace FishNet.Managing.Logging
{
    /// <summary>
    /// Configuration ScriptableObject specifying which data to log. Used in conjuction with NetworkManager.
    /// </summary>
    [CreateAssetMenu(fileName = "New LevelLoggingConfiguration", menuName = "FishNet/Logging/Level Logging Configuration")]
    public class LevelLoggingConfiguration : LoggingConfiguration
    {
        #region Serialized.
        /// <summary>
        /// True to add localtick to logs.
        /// </summary>
        [Tooltip("True to add localtick to logs.")]
        [SerializeField]
        private bool _addLocalTick;
        /// <summary>
        /// True to add timestamps to logs.
        /// </summary>
        [Tooltip("True to add timestamps to logs.")]
        [SerializeField]
        private bool _addTimestamps = true;
        /// <summary>
        /// True to add timestamps when in editor. False to only include timestamps in builds.
        /// </summary>
        [Tooltip("True to add timestamps when in editor. False to only include timestamps in builds.")]
        [SerializeField]
        private bool _enableTimestampsInEditor;
        /// <summary>
        /// Type of logging to use for development builds and editor.
        /// </summary>
        [Tooltip("Type of logging to use for development builds and editor.")]
        [SerializeField]
        private LoggingType _developmentLogging = LoggingType.Common;
        /// <summary>
        /// Type of logging to use for GUI builds.
        /// </summary>
        [Tooltip("Type of logging to use for GUI builds.")]
        [SerializeField]
        private LoggingType _guiLogging = LoggingType.Warning;
        /// <summary>
        /// Type of logging to use for headless builds.
        /// </summary>
        [Tooltip("Type of logging to use for headless builds.")]
        [SerializeField]
        private LoggingType _headlessLogging = LoggingType.Error;
        #endregion

        #region Private.
        /// <summary>
        /// True when initialized.
        /// </summary>
        private bool _initialized;
        /// <summary>
        /// Highest type which can be logged.
        /// </summary>
        private LoggingType _highestLoggingType = LoggingType.Off;
        /// <summary>
        /// Sequential stringbuilder for performance.
        /// </summary>
        private static StringBuilder _stringBuilder = new();
        #endregion

        [APIExclude]
        public void LoggingConstructor(bool loggingEnabled, LoggingType development, LoggingType gui, LoggingType headless)
        {
            IsEnabled = loggingEnabled;
            _developmentLogging = development;
            _guiLogging = gui;
            _headlessLogging = headless;
        }

        /// <summary>
        /// Initializes script for use.
        /// </summary>
        /// <param name = "manager"></param>
        public override void InitializeOnce()
        {
            byte currentHighest = (byte)LoggingType.Off;
#if UNITY_SERVER
            currentHighest = Math.Max(currentHighest, (byte)_headlessLogging);
#elif DEVELOPMENT
            currentHighest = Math.Max(currentHighest, (byte)_developmentLogging);
#else
            currentHighest = Math.Max(currentHighest, (byte)_guiLogging);
#endif
            _highestLoggingType = (LoggingType)currentHighest;
            _initialized = true;
        }

        /// <summary>
        /// True if can log for loggingType.
        /// </summary>
        /// <param name = "loggingType">Type of logging being filtered.</param>
        /// <returns></returns>
        public override bool CanLog(LoggingType loggingType)
        {
            if (!IsEnabled)
                return false;

            if (!_initialized)
            {
#if DEVELOPMENT
                if (Application.isPlaying)
                    NetworkManagerExtensions.LogError("CanLog called before being initialized.");
                else
                    return true;
#endif
                return false;
            }

            return (byte)loggingType <= (byte)_highestLoggingType;
        }

        /// <summary>
        /// Logs a common value if can log.
        /// </summary>
        public override void Log(string value)
        {
            if (CanLog(LoggingType.Common))
                Debug.Log(AddSettingsToLog(value));
        }

        /// <summary>
        /// Logs a warning value if can log.
        /// </summary>
        public override void LogWarning(string value)
        {
            if (CanLog(LoggingType.Warning))
                Debug.LogWarning(AddSettingsToLog(value));
        }

        /// <summary>
        /// Logs an error value if can log.
        /// </summary>
        public override void LogError(string value)
        {
            if (CanLog(LoggingType.Error))
                Debug.LogError(AddSettingsToLog(value));
        }

        /// <summary>
        /// Clones this logging configuration.
        /// </summary>
        /// <returns></returns>
        public override LoggingConfiguration Clone()
        {
            LevelLoggingConfiguration copy = CreateInstance<LevelLoggingConfiguration>();
            copy.LoggingConstructor(IsEnabled, _developmentLogging, _guiLogging, _headlessLogging);
            copy._addTimestamps = _addTimestamps;
            copy._addLocalTick = _addLocalTick;
            copy._enableTimestampsInEditor = _enableTimestampsInEditor;

            return copy;
        }

        /// <summary>
        /// Adds onto logging message if settings are enabled to.
        /// </summary>
        private string AddSettingsToLog(string value)
        {
            _stringBuilder.Clear();


            if (_addTimestamps && (!Application.isEditor || _enableTimestampsInEditor))
                _stringBuilder.Append($"[{DateTime.Now:yyyy.MM.dd HH:mm:ss}] ");

            if (_addLocalTick)
            {
                TimeManager tm = InstanceFinder.TimeManager;
                uint tick = tm == null ? TimeManager.UNSET_TICK : tm.LocalTick;
                _stringBuilder.Append($"LocalTick [{tick}] ");
            }

            // If anything was added onto string builder then add value, and set value to string builder.
            if (_stringBuilder.Length > 0)
            {
                _stringBuilder.Append(value);
                value = _stringBuilder.ToString();
            }

            return value;
        }
    }
}