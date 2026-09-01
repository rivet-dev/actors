using FishNet.Documenting;
using FishNet.Managing.Logging;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Managing
{
    public partial class NetworkManager : MonoBehaviour
    {
        #region Serialized.
        /// <summary>
        /// Logging configuration to use. When empty default logging settings will be used.
        /// </summary>
        [Tooltip("Logging configuration to use. When empty default logging settings will be used.")]
        [SerializeField]
        private LoggingConfiguration _logging;
        #endregion

        #region Const.
        private const string ERROR_LOGGING_PREFIX = "Error - ";
        private const string WARNING_LOGGING_PREFIX = "Warning - ";
        private const string COMMON_LOGGING_PREFIX = "Log - ";
        #endregion

        /// <summary>
        /// Initializes logging settings.
        /// </summary>
        private void InitializeLogging()
        {
            if (_logging == null)
                _logging = ScriptableObject.CreateInstance<LevelLoggingConfiguration>();
            else
                _logging = _logging.Clone();

            _logging.InitializeOnce();
        }

        /// <summary>
        /// True if can log for loggingType.
        /// </summary>
        /// <param name = "loggingType">Type of logging being filtered.</param>
        /// <returns></returns>
        internal bool InternalCanLog(LoggingType loggingType)
        {
            return _logging.CanLog(loggingType);
        }

        /// <summary>
        /// Performs a common log, should logging settings permit it.
        /// </summary>
        internal void InternalLog(string value)
        {
            _logging.Log(value);
        }

        /// <summary>
        /// Performs a log using the loggingType, should logging settings permit it.
        /// </summary>
        internal void InternalLog(LoggingType loggingType, string value)
        {
            if (loggingType == LoggingType.Common)
                _logging.Log(value);
            else if (loggingType == LoggingType.Warning)
                _logging.LogWarning(value);
            else if (loggingType == LoggingType.Error)
                _logging.LogError(value);
        }

        /// <summary>
        /// Performs a warning log, should logging settings permit it.
        /// </summary>
        internal void InternalLogWarning(string value)
        {
            _logging.LogWarning(value);
        }

        /// <summary>
        /// Performs an error log, should logging settings permit it.
        /// </summary>
        internal void InternalLogError(string value)
        {
            _logging.LogError(value);
        }
    }

    public static class NetworkManagerExtensions
    {
        /// <summary>
        /// True if can log for loggingType.
        /// </summary>
        internal static bool CanLog(this NetworkManager networkManager, LoggingType loggingType)
        {
            if (GetNetworkManager(ref networkManager))
                return networkManager.InternalCanLog(loggingType);
            else
                return false;
        }

        /// <summary>
        /// Performs a log using the loggingType, should logging settings permit it.
        /// </summary>
        public static void Log(this NetworkManager networkManager, LoggingType loggingType, string value)
        {
            if (loggingType == LoggingType.Common)
                networkManager.Log(value);
            else if (loggingType == LoggingType.Warning)
                networkManager.LogWarning(value);
            else if (loggingType == LoggingType.Error)
                networkManager.LogError(value);
        }

        /// <summary>
        /// Performs a common log, should logging settings permit it.
        /// </summary>
        public static void Log(this NetworkManager networkManager, string message)
        {
            if (GetNetworkManager(ref networkManager))
                networkManager.InternalLog(message);
            else
                Debug.Log(message);
        }

        /// <summary>
        /// Performs a warning log, should logging settings permit it.
        /// </summary>
        public static void LogWarning(this NetworkManager networkManager, string message)
        {
            if (GetNetworkManager(ref networkManager))
                networkManager.InternalLogWarning(message);
            else
                Debug.LogWarning(message);
        }

        /// <summary>
        /// Performs an error log, should logging settings permit it.
        /// </summary>
        public static void LogError(this NetworkManager networkManager, string message)
        {
            if (GetNetworkManager(ref networkManager))
                networkManager.InternalLogError(message);
            else
                Debug.LogError(message);
        }

        /// <summary>
        /// Gets a NetworkManager, first using a preferred option.
        /// </summary>
        /// <returns>True if a NetworkManager was found.</returns>
        private static bool GetNetworkManager(ref NetworkManager preferredNm)
        {
            if (preferredNm != null)
                return true;

            preferredNm = InstanceFinder.NetworkManager;
            return preferredNm != null;
        }

        #region Backwards compatibility.
        /// <summary>
        /// Performs a common log, should logging settings permit it.
        /// </summary>
        public static void Log(string msg) => Log(null, msg);

        /// <summary>
        /// Performs a warning log, should logging settings permit it.
        /// </summary>
        public static void LogWarning(string msg) => LogWarning(null, msg);

        /// <summary>
        /// Performs an error log, should logging settings permit it.
        /// </summary>
        public static void LogError(string msg) => LogError(null, msg);

        /// <summary>
        /// True if can log for loggingType.
        /// </summary>
        public static bool CanLog(LoggingType lt) => CanLog(null, lt);
        #endregion
    }
}