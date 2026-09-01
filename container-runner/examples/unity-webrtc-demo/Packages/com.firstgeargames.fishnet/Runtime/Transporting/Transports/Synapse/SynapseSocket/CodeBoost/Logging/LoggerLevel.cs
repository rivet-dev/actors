namespace CodeBoost.Logging
{

    /// <summary>
    /// What type of messages to log.
    /// </summary>
    public enum LoggerLevel : byte
    {
        /// <summary>
        /// Log error and higher.
        /// </summary>
        Error = 1,
        /// <summary>
        /// Log warning and higher.
        /// </summary>
        Warning = 2,
        /// <summary>
        /// Log information and higher.
        /// </summary>
        Information = 3,
        /// <summary>
        /// All logging is disabled.
        /// </summary>
        Disabled = 4
    }
}
