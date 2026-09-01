#pragma warning disable CS8632 // The annotation for nullable reference types should only be used in code within a '#nullable' annotations context.
using System;

namespace CodeBoost.Logging
{

    /// <summary>
    /// A logger which uses Console writing for messages.
    /// </summary>
    public class ConsoleLogger : ILogger
    {
        /// <summary>
        /// Returns the settings to use for this logger.
        /// </summary>
        /// <returns>Settings to use for logger.</returns>
        public LoggerSetting GetLoggerSetting() => LoggerSetting.LoggerServiceSetting;

        /// <summary>
        /// Disables always including stacktrace in development environments.
        /// </summary>
        /// <returns>True to disable always including stacktrace in development environments.</returns>
        /// <remarks>Even with unconditional inclusions disabled stacktrace will still be included for higher level log calls.</remarks>
        public bool DisableUnconditionalDevelopmentStacktrace() => true;
            
        /// <summary>
        /// Logs a message as information.
        /// </summary>
        public void LogInformation(string message) => Console.WriteLine($"Information :: {Logger.AddStackTraceIfDevelopment(message)}");

        /// <summary>
        /// Logs a message as warning.
        /// </summary>
        public void LogWarning(string message) => Console.WriteLine($"Warning :: {Logger.AddStackTraceIfDevelopment(message)}");

        /// <summary>
        /// Logs a message as error.
        /// </summary>
        public void LogError(string message) => Console.WriteLine($"Error :: {Logger.AddStackTrace(message)}");
    }
}
