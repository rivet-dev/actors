using System.Diagnostics;
using CodeBoost.Environment;
using CodeBoost.Extensions;
using System;

#pragma warning disable CS8632 // The annotation for nullable reference types should only be used in code within a '#nullable' annotations context.
#pragma warning disable CS0067 // Event is never used

namespace CodeBoost.Logging
{

    public static class LoggingService
    {
        /// <summary>
        /// Called when Logger is set.
        /// </summary>
        public static event LoggerSetEventHandler? LoggerSet;

        public delegate void LoggerSetEventHandler(ILogger logger);

        /// <summary>
        /// ILogger to use.
        /// </summary>
        public static ILogger? Logger;
        /// <summary>
        /// Cached value of logging level for performance.
        /// </summary>
        private static byte _loggerLevelAsUnderlyingType;
        /// <summary>
        /// Message when trying to access Logger when there is not an instance created.
        /// </summary>
        private static readonly string LoggerIsNullMessage = $"[{nameof(LoggingService)}] is null. Use [{nameof(LoggingService)}] to set a service.";

        static LoggingService()
        {
            UseLogger(new ConsoleLogger());
        }

        /// <summary>
        /// Specifies which ILogger to use.
        /// </summary>
        public static void UseLogger(ILogger logger)
        {
            Logger = logger;
            CacheLoggerLevelAsUnderlyingType();

            LoggerSet?.Invoke(logger);
        }

            
        public static bool DisableUnconditionalDevelopmentStacktrace()
        {
            if (Logger is not null)
                return Logger.DisableUnconditionalDevelopmentStacktrace();

            throw new(LoggerIsNullMessage);
        }

        /// <summary>
        /// Logs a message as information.
        /// </summary>
        public static void LogInformation(string message)
        {
            if (Logger is not null)
            {
                if (_loggerLevelAsUnderlyingType < (byte)LoggerLevel.Information)
                    return;

                Logger.LogInformation(message);
            }
            else
            {
                throw new(LoggerIsNullMessage);
            }
        }

        /// <summary>
        /// Logs a message as warning.
        /// </summary>
        public static void LogWarning(string message)
        {
            if (Logger is not null)
            {
                if (_loggerLevelAsUnderlyingType < (byte)LoggerLevel.Warning)
                    return;

                Logger.LogWarning(message);
            }
            else
            {
                throw new(LoggerIsNullMessage);
            }
        }

        /// <summary>
        /// Logs a message as error.
        /// </summary>
        public static void LogError(string message)
        {
            if (Logger is not null)
            {
                if (_loggerLevelAsUnderlyingType < (byte)LoggerLevel.Error)
                    return;

                Logger.LogError(message);
            }
            else
            {
                throw new(LoggerIsNullMessage);
            }
        }

        /// <summary>
        /// Gets logger level for the current environment.
        /// </summary>
        /// <returns></returns>
        private static LoggerLevel GetLoggerLevel()
        {
            IApplicationState? applicationState = ApplicationStateService.ApplicationState;
            ILogger? logger = Logger;

            // Missing dependencies.
            if (logger is null || applicationState is null)
                return LoggerLevel.Disabled;

            LoggerSetting loggingSetting = logger.GetLoggerSetting();
            bool isEditor = applicationState.IsEditor();
            if (isEditor)
                return loggingSetting.Editor;

            bool isDevelopment = applicationState.IsDevelopment();
            if (isDevelopment)
                return loggingSetting.DevelopmentBuilds;

            // If here then is release builds.
            return loggingSetting.ReleaseBuilds;
        }

        /// <summary>
        /// Gets logger level as the underlying type.
        /// </summary>
        /// <returns></returns>
        private static byte GetLoggerLevelAsUnderlyingType() => (byte)GetLoggerLevel();

        /// <summary>
        /// Caches the logger level to use.
        /// </summary>
        private static void CacheLoggerLevelAsUnderlyingType() => _loggerLevelAsUnderlyingType = GetLoggerLevelAsUnderlyingType();
    }
}
