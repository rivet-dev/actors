using FishNet.Documenting;
using System;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Managing.Logging
{
    /// <summary>
    /// Base for logging configurations.
    /// </summary>
    public abstract class LoggingConfiguration : ScriptableObject
    {
        #region Serialized.
        /// <summary>
        /// True to use logging features. False to disable all logging.
        /// </summary>
        [Tooltip("True to use logging features. False to disable all logging.")]
        public bool IsEnabled = true;
        [Obsolete("Use IsEnabled.")] // Remove V5
        public bool LoggingEnabled
        {
            get => IsEnabled;
            set => IsEnabled = value;
        }
        #endregion

        /// <summary>
        /// Initializes script for use.
        /// </summary>
        /// <param name = "manager"></param>
        public virtual void InitializeOnce() { }

        /// <summary>
        /// True if can log for loggingType.
        /// </summary>
        /// <param name = "loggingType">Type of logging being filtered.</param>
        /// <returns></returns>
        public abstract bool CanLog(LoggingType loggingType);

        /// <summary>
        /// Logs a common value if can log.
        /// </summary>
        public abstract void Log(string value);

        /// <summary>
        /// Logs a warning value if can log.
        /// </summary>
        public abstract void LogWarning(string value);

        /// <summary>
        /// Logs an error value if can log.
        /// </summary>
        public abstract void LogError(string value);

        /// <summary>
        /// Clones this logging configuration.
        /// </summary>
        /// <returns></returns>
        public abstract LoggingConfiguration Clone();
    }
}