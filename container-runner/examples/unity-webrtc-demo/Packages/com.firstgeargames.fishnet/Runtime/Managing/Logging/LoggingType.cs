namespace FishNet.Managing.Logging
{
    /// <summary>
    /// Type of logging being filtered.
    /// </summary>
    public enum LoggingType : byte
    {
        /// <summary>
        /// Disable logging.
        /// </summary>
        Off = 0,
        /// <summary>
        /// Only log errors.
        /// </summary>
        Error = 1,
        /// <summary>
        /// Log warnings and errors.
        /// </summary>
        Warning = 2,
        /// <summary>
        /// Log all common activities, warnings, and errors.
        /// </summary>
        Common = 3
    }
}