namespace CodeBoost.Logging
{

    /// <summary>
    /// What type of messages to log.
    /// </summary>
    public class LoggerSetting
    {
        /// <summary>
        /// Logging level for editor.
        /// </summary>
        // Do not make readonly so that the field may be serialized.
        // ReSharper disable once FieldCanBeMadeReadOnly.Global
        public LoggerLevel Editor = LoggerLevel.Information;
        /// <summary>
        /// Logging level for development builds.
        /// </summary>
        // Do not make readonly so that the field may be serialized.
        // ReSharper disable once FieldCanBeMadeReadOnly.Global
        public LoggerLevel DevelopmentBuilds = LoggerLevel.Warning;
        /// <summary>
        /// Logging level for release builds.
        /// </summary>
        // Do not make readonly so that the field may be serialized.
        // ReSharper disable once FieldCanBeMadeReadOnly.Global
        public LoggerLevel ReleaseBuilds = LoggerLevel.Error;
            
        /// <summary>
        /// Default settings to use.
        /// </summary>
        public static readonly LoggerSetting LoggerServiceSetting = new()
        {
            DevelopmentBuilds = LoggerLevel.Information,
            Editor = LoggerLevel.Information,
            ReleaseBuilds = LoggerLevel.Error,
        };
    }
}
