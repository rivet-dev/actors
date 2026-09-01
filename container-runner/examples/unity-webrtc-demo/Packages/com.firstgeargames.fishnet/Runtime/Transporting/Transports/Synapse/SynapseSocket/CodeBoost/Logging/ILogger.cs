namespace CodeBoost.Logging
{

    public interface ILogger
    {
        /// <summary>
        /// Returns the settings to use for this logger.
        /// </summary>
        /// <returns>Settings to use for logger.</returns>
        public LoggerSetting GetLoggerSetting();

        /// <summary>
        /// Disables always including stacktrace in development environments.
        /// </summary>
        /// <returns>True to disable always including stacktrace in development environments.</returns>
        /// <remarks>Even with unconditional inclusions disabled stacktrace will still be included for higher level log calls.</remarks>
        public bool DisableUnconditionalDevelopmentStacktrace();
            
        /// <summary>
        /// Logs a message as information.
        /// </summary>
        public void LogInformation(string message);

        /// <summary>
        /// Logs a message as warning.
        /// </summary>
        public void LogWarning(string message);

        /// <summary>
        /// Logs a message as error.
        /// </summary>
        public void LogError(string message);
    }
}
